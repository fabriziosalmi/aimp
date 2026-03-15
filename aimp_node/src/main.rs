pub mod protocol;
pub mod crypto;
pub mod crdt;
pub mod network;
pub mod ai_bridge;
pub mod dashboard;
pub mod config;
pub mod event;

use std::sync::Arc;
use std::net::SocketAddr;
use std::collections::BTreeMap;
use tokio::sync::mpsc;
use axum::{routing::get, Router};
use prometheus::Encoder;

use crypto::Identity;
use network::GossipNetwork;
use protocol::{AimpEnvelope, AimpData, OpCode};
use event::SystemEvent;
use crdt::{CrdtActor, CrdtHandle, PersistentStore};
use ai_bridge::AiEngine;
use dashboard::Dashboard;
use event::metrics::GLOBAL_METRICS;
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
async fn main() -> Result<(), Box<dyn std::error::Error>> {
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
    let store = match PersistentStore::open("./aimp_state", identity.noise_static_secret.to_bytes()) {
        Ok(s) => Some(s),
        Err(e) => {
            eprintln!("⚠️ WARNING: Could not open persistent store: {}. Running in-memory.", e);
            None
        }
    };

    let (crdt_tx, crdt_rx) = mpsc::channel(100);
    let crdt_handle = CrdtHandle::new(crdt_tx);
    let crdt_actor = CrdtActor::new(crdt_rx, store, Some(log_tx.clone()), cfg.quorum_threshold);
    
    // Spawn Actor
    tokio::spawn(async move {
        crdt_actor.run().await;
    });

    // 3. Local Deterministic AI Engine (No external dependencies)
    let ai_engine = Arc::new(AiEngine::new(Some(log_tx.clone()))?);

    // 4. Networking: Professional Error Handling for Port Conflicts
    let (rx_channel_tx, mut rx_channel_rx) = mpsc::channel::<(AimpEnvelope, SocketAddr)>(1000);
    
    let network_result = GossipNetwork::new(port, identity.clone(), Some(log_tx.clone())).await;
    
    let mut network = match network_result {
        Ok(n) => n,
        Err(e) => {
            if e.to_string().contains("Address already in use") {
                eprintln!("❌ ERROR: Port {} is already in use by another process.", args.port);
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
            .route("/metrics", get(|| async {
                let encoder = prometheus::TextEncoder::new();
                let mut buffer = Vec::new();
                encoder.encode(&GLOBAL_METRICS.registry.gather(), &mut buffer).unwrap();
                String::from_utf8(buffer).unwrap()
            }))
            .route("/health", get(move || {
                let s = state_hc.clone();
                async move {
                    let check = tokio::time::timeout(
                        std::time::Duration::from_millis(200), 
                        s.crdt_handle.get_merkle_root()
                    ).await;
                    
                    if check.is_ok() { "OK" } else { "ERROR: Subsystem Timeout" }
                }
            }));

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
                    OpCode::Infer   => handle_infer(envelope.data, inner_state).await,
                    _               => {},
                }
            });
        }
    });

    let node_display_id = args.name.unwrap_or_else(|| my_pubkey_hex[..8].to_string());
    let dash = Dashboard::new(node_display_id, crdt_handle, log_rx);
    
    let _ = log_tx.send(SystemEvent::Status("Node initialized".into())).await;
    let _ = log_tx.send(SystemEvent::Status(format!("Networking active on UDP:{}", args.port))).await;
    
    dash.run()?;

    Ok(())
}

async fn handle_sync(data: AimpData, state: Arc<NodeState>) {
    let incoming_nodes: Result<Vec<crdt::DagNode>, _> = rmp_serde::from_slice(&data.payload);
    if let Ok(nodes) = incoming_nodes {
        let added = state.crdt_handle.merge_nodes(nodes).await;
        if added > 0 {
            let _ = state.log_tx.try_send(SystemEvent::StateMerged { nodes_added: added });
        }
    }
}

async fn handle_infer(data: AimpData, state: Arc<NodeState>) {
    let prompt = String::from_utf8_lossy(&data.payload).to_string();
    let _ = state.log_tx.send(SystemEvent::AiInference { 
        prompt: prompt.clone(), 
        decision: "Pending...".into() 
    }).await;
    
    if let Ok(res) = state.ai_engine.run_deterministic_inference(&prompt, "{}").await {
        let mut ordered_map = BTreeMap::new();
        ordered_map.insert("action".to_string(), serde_json::json!(res.action_required));
        ordered_map.insert("status".to_string(), serde_json::json!(res.status));

        if let Ok(deterministic_json) = serde_json::to_string(&ordered_map) {
            let data_hash = crate::crypto::SecurityFirewall::hash(deterministic_json.as_bytes());

            // Create the signature for the new mutation
            if let Ok(sig_result) = state.identity.sign(data.clone()) {
                // Causal Continuity: Increment and inject local clock
                let current_tick = state.node_clock.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                let mut vclock = BTreeMap::new();
                vclock.insert(hex::encode(state.identity.node_id())[..8].to_string(), current_tick);

                // v0.4.0: Construct AiEvidence for auditable trail
                let evidence = crate::crdt::merkle_dag::AiEvidence {
                    prompt: prompt.clone(),
                    decision: deterministic_json.clone(),
                    model_hash: state.ai_engine.get_model_hash(),
                    timestamp: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
                };

                state.crdt_handle.append_mutation(
                    data_hash, 
                    sig_result.signature, 
                    vclock,
                    Some(evidence)
                ).await;
                let _ = state.log_tx.send(SystemEvent::MutationCommitted { 
                    hash: hex::encode(data_hash), 
                    author: hex::encode(state.identity.node_id())[..8].to_string() 
                }).await;
            } else {
                let _ = state.log_tx.send(SystemEvent::Status("[ERROR] Failed to sign mutation".into())).await;
            }
        } else {
            let _ = state.log_tx.send(SystemEvent::Status("[ERROR] Failed to serialize AI decision".into())).await;
        }
    }
}
