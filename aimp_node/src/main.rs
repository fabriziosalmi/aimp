use axum::{routing::get, Router};
use prometheus::Encoder;
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;

use aimp_node::ai_bridge::AiEngine;
use aimp_node::config;
use aimp_node::crdt::{self, CrdtActor, CrdtHandle, PersistentStore};
use aimp_node::crypto::Identity;
use aimp_node::dashboard::Dashboard;
use aimp_node::event::metrics::GLOBAL_METRICS;
use aimp_node::event::SystemEvent;
use aimp_node::network::GossipNetwork;
use aimp_node::protocol::{AimpData, AimpEnvelope, OpCode, Payload};
use clap::Parser;

/// AIMP Mesh Node: Reference implementation of the AI Mesh Protocol.
/// A zero-trust, Merkle-CRDT based state synchronization engine for deterministic AI.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// UDP Port to listen on (broadcast enabled)
    #[arg(short, long, default_value_t = config::DEFAULT_PORT)]
    port: u16,

    /// Optional node name for the Mission Log
    #[arg(short, long)]
    name: Option<String>,
}
struct NodeState {
    identity: Arc<Identity>,
    crdt_handle: CrdtHandle,
    ai_engine: Arc<AiEngine>,
    log_tx: mpsc::Sender<SystemEvent>,
    node_clock: std::sync::atomic::AtomicU64,
}

#[tokio::main]
async fn main() -> aimp_node::AimpResult<()> {
    let args = Args::parse();

    // 0. Load Dynamic Configuration
    let cfg = config::AimpConfig::new().unwrap_or_else(|e| {
        eprintln!("⚠️ WARNING: Invalid config, using defaults: {}", e);
        config::AimpConfig::new().unwrap() // Fallback to hardcoded defaults
    });

    let port = args.port; // Override cfg with CLI if provided

    // 1. Initial Identity & Systems
    let identity = Arc::new(Identity::new());
    let my_pubkey_hex = hex::encode(identity.node_id());
    // 2. Logging Channel (Required for system observability)
    let (log_tx, log_rx) = mpsc::channel::<SystemEvent>(cfg.network_log_capacity);

    // 3. Systems: CRDT Actor Initialization with Persistence
    let store = match PersistentStore::open("./aimp_state", identity.noise_static_secret.to_bytes())
    {
        Ok(s) => Some(s),
        Err(e) => {
            eprintln!(
                "⚠️ WARNING: Could not open persistent store: {}. Running in-memory.",
                e
            );
            None
        }
    };

    let (crdt_tx, crdt_rx) = mpsc::channel(100);
    let crdt_handle = CrdtHandle::new(crdt_tx);
    let crdt_actor = CrdtActor::new(
        crdt_rx,
        store,
        Some(log_tx.clone()),
        cfg.quorum_threshold,
        cfg.gc_mutation_threshold,
    );

    // Spawn Actor
    tokio::spawn(async move {
        crdt_actor.run().await;
    });

    // 3. Local Deterministic AI Engine (No external dependencies)
    let ai_engine = Arc::new(AiEngine::new(Some(log_tx.clone()))?);

    // 4. Networking: Professional Error Handling for Port Conflicts
    let (rx_channel_tx, mut rx_channel_rx) = mpsc::channel::<(AimpEnvelope, SocketAddr)>(1000);

    let network_result = GossipNetwork::new(
        port,
        identity.clone(),
        Some(log_tx.clone()),
        cfg.noise_required,
        cfg.peer_rate_limit,
        cfg.peer_rate_burst,
    )
    .await;

    let mut network = match network_result {
        Ok(n) => n,
        Err(e) => {
            if e.to_string().contains("Address already in use") {
                eprintln!(
                    "❌ ERROR: Port {} is already in use by another process.",
                    args.port
                );
                eprintln!("👉 SUGGESTION: Run with '--port <NEW_PORT>' to avoid conflicts.");
                std::process::exit(1);
            }
            return Err(e);
        }
    };

    tokio::spawn(async move {
        network.listen(rx_channel_tx).await;
    });

    let state = Arc::new(NodeState {
        identity: identity.clone(),
        crdt_handle: crdt_handle.clone(),
        ai_engine: ai_engine.clone(),
        log_tx: log_tx.clone(),
        node_clock: std::sync::atomic::AtomicU64::new(0),
    });

    // 2.1 Metrics & Health Server (Prometheus)
    let state_hc = state.clone();
    tokio::spawn(async move {
        let app = Router::new()
            .route(
                "/metrics",
                get(|| async {
                    let encoder = prometheus::TextEncoder::new();
                    let mut buffer = Vec::new();
                    encoder
                        .encode(&GLOBAL_METRICS.registry.gather(), &mut buffer)
                        .unwrap();
                    String::from_utf8(buffer).unwrap()
                }),
            )
            .route(
                "/health",
                get(move || {
                    let s = state_hc.clone();
                    async move {
                        let timeout = std::time::Duration::from_millis(200);
                        let mut checks = serde_json::Map::new();
                        let mut healthy = true;

                        // Check CRDT actor responsiveness
                        match tokio::time::timeout(timeout, s.crdt_handle.get_merkle_root()).await {
                            Ok(root) => {
                                checks.insert(
                                    "crdt".into(),
                                    serde_json::json!({
                                        "status": "ok",
                                        "merkle_root": hex::encode(root)
                                    }),
                                );
                            }
                            Err(_) => {
                                checks.insert(
                                    "crdt".into(),
                                    serde_json::json!({"status": "timeout"}),
                                );
                                healthy = false;
                            }
                        }

                        // Check log channel capacity (proxy for system load)
                        let log_capacity = s.log_tx.capacity();
                        if log_capacity == 0 {
                            checks.insert(
                                "log_channel".into(),
                                serde_json::json!({"status": "full"}),
                            );
                            healthy = false;
                        } else {
                            checks.insert(
                                "log_channel".into(),
                                serde_json::json!({
                                    "status": "ok",
                                    "available": log_capacity
                                }),
                            );
                        }

                        let result = serde_json::json!({
                            "healthy": healthy,
                            "checks": checks
                        });
                        (
                            if healthy {
                                axum::http::StatusCode::OK
                            } else {
                                axum::http::StatusCode::SERVICE_UNAVAILABLE
                            },
                            result.to_string(),
                        )
                    }
                }),
            );

        let addr = format!("0.0.0.0:{}", cfg.metrics_port);
        let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
        let _ = axum::serve(listener, app).await;
    });

    let state_logic = state.clone();
    tokio::spawn(async move {
        while let Some((envelope, _peer_ip)) = rx_channel_rx.recv().await {
            let op = envelope.data.op;
            let inner_state = state_logic.clone();

            tokio::spawn(async move {
                match op {
                    OpCode::SyncRes => handle_sync(envelope.data, inner_state).await,
                    OpCode::Infer => handle_infer(envelope.data, inner_state).await,
                    _ => {}
                }
            });
        }
    });

    let node_display_id = args.name.unwrap_or_else(|| my_pubkey_hex[..8].to_string());
    let dash = Dashboard::new(node_display_id, crdt_handle, log_rx);

    let _ = log_tx
        .send(SystemEvent::Status("Node initialized".into()))
        .await;
    let _ = log_tx
        .send(SystemEvent::Status(format!(
            "Networking active on UDP:{}",
            args.port
        )))
        .await;

    // Graceful shutdown: listen for signals in parallel with dashboard
    let shutdown_log_tx = log_tx.clone();
    let shutdown_handle = tokio::spawn(async move {
        let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("Failed to register SIGTERM handler");

        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                let _ = shutdown_log_tx.send(SystemEvent::Status("Received SIGINT, shutting down...".into())).await;
            }
            _ = sigterm.recv() => {
                let _ = shutdown_log_tx.send(SystemEvent::Status("Received SIGTERM, shutting down...".into())).await;
            }
        }
    });

    // Dashboard blocks the main thread; when it exits (user presses 'q' or signal arrives),
    // we proceed with cleanup
    let _ = dash.run();

    // Cancel the signal handler if dashboard exited first
    shutdown_handle.abort();

    // Graceful shutdown with 5-second timeout
    let shutdown_fut = async {
        let _ = log_tx
            .send(SystemEvent::Status("Flushing state to disk...".into()))
            .await;
        // Drop the log_tx to signal downstream consumers
        drop(log_tx);
    };

    if tokio::time::timeout(std::time::Duration::from_secs(5), shutdown_fut)
        .await
        .is_err()
    {
        eprintln!("Shutdown timed out after 5s, forcing exit");
    }

    Ok(())
}

async fn handle_sync(data: AimpData, state: Arc<NodeState>) {
    let timer = GLOBAL_METRICS.sync_duration.start_timer();
    if let Payload::SyncResponse(nodes) = Payload::decode(data.op, &data.payload) {
        let added = state.crdt_handle.merge_nodes(nodes).await;
        if added > 0 {
            let _ = state
                .log_tx
                .try_send(SystemEvent::StateMerged { nodes_added: added });
        }
    }
    timer.observe_duration();
}

async fn handle_infer(data: AimpData, state: Arc<NodeState>) {
    let timer = GLOBAL_METRICS.inference_duration.start_timer();
    let prompt = match Payload::decode(data.op, &data.payload) {
        Payload::InferPrompt(s) => s,
        _ => return,
    };
    let _ = state
        .log_tx
        .send(SystemEvent::AiInference {
            prompt: prompt.clone(),
            decision: "Pending...".into(),
        })
        .await;

    if let Ok(res) = state
        .ai_engine
        .run_deterministic_inference(&prompt, "{}")
        .await
    {
        let mut ordered_map = BTreeMap::new();
        ordered_map.insert("action".to_string(), serde_json::json!(res.action_required));
        ordered_map.insert("status".to_string(), serde_json::json!(res.status));

        if let Ok(deterministic_json) = serde_json::to_string(&ordered_map) {
            let data_hash =
                aimp_node::crypto::SecurityFirewall::hash(deterministic_json.as_bytes());

            // Create the signature for the new mutation
            if let Ok(sig_result) = state.identity.sign(data.clone()) {
                // Causal Continuity: Increment and inject local clock
                let current_tick = state
                    .node_clock
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
                    + 1;
                let mut vclock = BTreeMap::new();
                vclock.insert(
                    hex::encode(state.identity.node_id())[..8].to_string(),
                    current_tick,
                );

                // v0.4.0: Construct AiEvidence for auditable trail
                let evidence = crate::crdt::merkle_dag::AiEvidence {
                    prompt: prompt.clone(),
                    decision: deterministic_json.clone(),
                    model_hash: state.ai_engine.get_model_hash(),
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs(),
                };

                state
                    .crdt_handle
                    .append_mutation(data_hash, sig_result.signature, vclock, Some(evidence))
                    .await;
                let _ = state
                    .log_tx
                    .send(SystemEvent::MutationCommitted {
                        hash: hex::encode(data_hash),
                        author: hex::encode(state.identity.node_id())[..8].to_string(),
                    })
                    .await;
            } else {
                let _ = state
                    .log_tx
                    .send(SystemEvent::Status(
                        "[ERROR] Failed to sign mutation".into(),
                    ))
                    .await;
            }
        } else {
            let _ = state
                .log_tx
                .send(SystemEvent::Status(
                    "[ERROR] Failed to serialize AI decision".into(),
                ))
                .await;
        }
    }
    timer.observe_duration();
}
