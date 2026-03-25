///! Network impairment benchmark for AIMP
///!
///! Simulates degraded network conditions (packet loss, latency, jitter,
///! partitions) and measures CRDT convergence behavior.
///!
///! Run: cargo run --release --manifest-path aimp_node/Cargo.toml --example bench_netem

use aimp_node::crdt::merkle_dag::{DagNode, MerkleCrdtEngine};
use aimp_node::crypto::{Identity, SecurityFirewall};
use aimp_node::protocol::{AimpData, OpCode};
use rand::prelude::*;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

const NUM_NODES: usize = 5;
const MUTATIONS_PER_NODE: usize = 50;

fn create_mutation(
    engine: &mut MerkleCrdtEngine,
    identity: &Identity,
    data: &[u8],
    node_id: &str,
    tick: usize,
) -> [u8; 32] {
    let data_hash = SecurityFirewall::hash(data);
    let sig = identity
        .sign(AimpData {
            v: 1,
            op: OpCode::Ping,
            ttl: 3,
            origin_pubkey: identity.node_id(),
            vclock: BTreeMap::new(),
            payload: data.to_vec(),
        })
        .unwrap()
        .signature;
    let mut vclock = BTreeMap::new();
    vclock.insert(node_id.to_string(), tick as u64);
    engine.append_mutation(data_hash, sig, vclock, None)
}

/// Recompute heads from scratch after inserting nodes.
fn recompute_heads(engine: &mut MerkleCrdtEngine) {
    let mut has_children = std::collections::HashSet::new();
    for (_, node) in engine.arena.get_all_iter() {
        for p in &node.parents {
            has_children.insert(*p);
        }
    }
    engine.heads.clear();
    for (hash, _) in engine.arena.get_all_iter() {
        if !has_children.contains(hash) {
            engine.heads.insert(*hash);
        }
    }
    engine.invalidate_root();
}

/// Perform a single sync round between all node pairs with simulated network conditions.
/// Returns (nodes_transferred, simulated_latency_us).
fn sync_round_with_impairment(
    engines: &mut Vec<MerkleCrdtEngine>,
    loss_pct: f64,
    _latency_us: u64,
    _jitter_us: u64,
    rng: &mut impl Rng,
) -> (usize, u64) {
    let n = engines.len();
    let mut transferred = 0;
    let mut total_simulated_latency = 0u64;

    for i in 0..n {
        for j in 0..n {
            if i == j {
                continue;
            }

            // Simulate packet loss: skip this sync pair
            if rng.gen::<f64>() < loss_pct / 100.0 {
                continue;
            }

            // Simulate latency + jitter
            let actual_latency = if _latency_us > 0 {
                let jitter = if _jitter_us > 0 {
                    rng.gen_range(0.._jitter_us * 2) as i64 - _jitter_us as i64
                } else {
                    0
                };
                (_latency_us as i64 + jitter).max(0) as u64
            } else {
                0
            };
            total_simulated_latency += actual_latency;

            // Collect src nodes
            let src_nodes: Vec<([u8; 32], DagNode)> = engines[i]
                .arena
                .get_all_iter()
                .map(|(h, n)| (*h, n.clone()))
                .collect();

            let dst = &mut engines[j];
            let mut added = false;
            for (hash, node) in &src_nodes {
                if !dst.arena.contains(hash) {
                    dst.arena.insert(*hash, node.clone());
                    transferred += 1;
                    added = true;
                }
            }
            if added {
                recompute_heads(dst);
            }
        }
    }

    (transferred, total_simulated_latency)
}

fn check_converged(engines: &mut Vec<MerkleCrdtEngine>) -> (bool, usize) {
    let roots: Vec<_> = engines.iter_mut().map(|e| e.get_merkle_root()).collect();
    let distinct = roots.iter().collect::<std::collections::HashSet<_>>().len();
    (distinct == 1, distinct)
}

struct NetemResult {
    scenario: String,
    converged: bool,
    rounds: usize,
    elapsed: Duration,
    nodes_transferred: usize,
    distinct_roots_final: usize,
}

fn run_scenario(
    scenario: &str,
    loss_pct: f64,
    latency_us: u64,
    jitter_us: u64,
    max_rounds: usize,
) -> NetemResult {
    let mut rng = rand::thread_rng();

    // Create nodes and independent mutations
    let mut engines: Vec<MerkleCrdtEngine> = (0..NUM_NODES)
        .map(|_| MerkleCrdtEngine::default())
        .collect();
    let identities: Vec<Identity> = (0..NUM_NODES).map(|_| Identity::new()).collect();

    for (i, engine) in engines.iter_mut().enumerate() {
        for tick in 0..MUTATIONS_PER_NODE {
            let data = format!("netem-n{}-m{}", i, tick);
            create_mutation(engine, &identities[i], data.as_bytes(), &format!("n{i}"), tick + 1);
        }
    }

    // Sync with impairment
    let start = Instant::now();
    let mut total_transferred = 0;
    let mut rounds = 0;

    loop {
        rounds += 1;
        let (transferred, _sim_latency) =
            sync_round_with_impairment(&mut engines, loss_pct, latency_us, jitter_us, &mut rng);
        total_transferred += transferred;

        let (converged, distinct) = check_converged(&mut engines);
        if converged {
            return NetemResult {
                scenario: scenario.to_string(),
                converged: true,
                rounds,
                elapsed: start.elapsed(),
                nodes_transferred: total_transferred,
                distinct_roots_final: 1,
            };
        }

        if transferred == 0 || rounds >= max_rounds {
            return NetemResult {
                scenario: scenario.to_string(),
                converged: false,
                rounds,
                elapsed: start.elapsed(),
                nodes_transferred: total_transferred,
                distinct_roots_final: distinct,
            };
        }
    }
}

fn run_partition_scenario(
    partition_duration_rounds: usize,
    loss_pct_during_merge: f64,
) -> NetemResult {
    let mut rng = rand::thread_rng();
    let scenario = format!(
        "Partition {}R then merge (loss={:.0}%)",
        partition_duration_rounds, loss_pct_during_merge
    );

    let mut engines: Vec<MerkleCrdtEngine> = (0..NUM_NODES)
        .map(|_| MerkleCrdtEngine::default())
        .collect();
    let identities: Vec<Identity> = (0..NUM_NODES).map(|_| Identity::new()).collect();

    // Phase 1: shared baseline (10 mutations from node 0, synced to all)
    for tick in 0..10 {
        let data = format!("base-{}", tick);
        create_mutation(&mut engines[0], &identities[0], data.as_bytes(), "n0", tick + 1);
    }
    for j in 1..NUM_NODES {
        let src_nodes: Vec<_> = engines[0]
            .arena
            .get_all_iter()
            .map(|(h, n)| (*h, n.clone()))
            .collect();
        for (hash, node) in &src_nodes {
            engines[j].arena.insert(*hash, node.clone());
        }
        recompute_heads(&mut engines[j]);
    }

    // Phase 2: Partition — Group A [0,1] and Group B [2,3,4]
    // Each group creates mutations independently for N rounds
    for round in 0..partition_duration_rounds {
        // Group A: node 0 mutates
        let data = format!("partA-r{}", round);
        create_mutation(
            &mut engines[0],
            &identities[0],
            data.as_bytes(),
            "n0",
            100 + round,
        );

        // Sync within A (0 → 1)
        let src: Vec<_> = engines[0]
            .arena
            .get_all_iter()
            .map(|(h, n)| (*h, n.clone()))
            .collect();
        let mut added = false;
        for (hash, node) in &src {
            if !engines[1].arena.contains(hash) {
                engines[1].arena.insert(*hash, node.clone());
                added = true;
            }
        }
        if added {
            recompute_heads(&mut engines[1]);
        }

        // Group B: node 2 mutates
        let data = format!("partB-r{}", round);
        create_mutation(
            &mut engines[2],
            &identities[2],
            data.as_bytes(),
            "n2",
            100 + round,
        );

        // Sync within B (2 → 3, 2 → 4)
        for j in [3usize, 4] {
            let src: Vec<_> = engines[2]
                .arena
                .get_all_iter()
                .map(|(h, n)| (*h, n.clone()))
                .collect();
            let mut added = false;
            for (hash, node) in &src {
                if !engines[j].arena.contains(hash) {
                    engines[j].arena.insert(*hash, node.clone());
                    added = true;
                }
            }
            if added {
                recompute_heads(&mut engines[j]);
            }
        }
    }

    // Phase 3: Partition heals — full mesh sync with possible loss
    let start = Instant::now();
    let mut total_transferred = 0;
    let mut rounds = 0;

    loop {
        rounds += 1;
        let (transferred, _) = sync_round_with_impairment(
            &mut engines,
            loss_pct_during_merge,
            0,
            0,
            &mut rng,
        );
        total_transferred += transferred;

        let (converged, distinct) = check_converged(&mut engines);
        if converged {
            return NetemResult {
                scenario,
                converged: true,
                rounds,
                elapsed: start.elapsed(),
                nodes_transferred: total_transferred,
                distinct_roots_final: 1,
            };
        }

        if transferred == 0 || rounds >= 50 {
            return NetemResult {
                scenario,
                converged: false,
                rounds,
                elapsed: start.elapsed(),
                nodes_transferred: total_transferred,
                distinct_roots_final: distinct,
            };
        }
    }
}

fn main() {
    println!("AIMP Network Impairment Benchmark (netem simulation)");
    println!("=====================================================");
    println!("Nodes: {NUM_NODES}, Mutations/node: {MUTATIONS_PER_NODE}\n");

    // Baseline (no impairment)
    let results = vec![
        run_scenario("Baseline (0% loss, 0 latency)", 0.0, 0, 0, 50),
        run_scenario("5% packet loss", 5.0, 0, 0, 50),
        run_scenario("10% packet loss", 10.0, 0, 0, 50),
        run_scenario("20% packet loss", 20.0, 0, 0, 50),
        run_scenario("30% packet loss", 30.0, 0, 0, 50),
        run_scenario("50% packet loss", 50.0, 0, 0, 100),
        run_scenario("10ms latency", 0.0, 10_000, 0, 50),
        run_scenario("50ms latency", 0.0, 50_000, 0, 50),
        run_scenario("100ms latency + 20ms jitter", 0.0, 100_000, 20_000, 50),
        run_scenario("10% loss + 50ms latency", 10.0, 50_000, 10_000, 50),
        run_scenario("20% loss + 100ms latency + 30ms jitter", 20.0, 100_000, 30_000, 50),
    ];

    // Partition scenarios
    let partition_results = vec![
        run_partition_scenario(10, 0.0),
        run_partition_scenario(10, 10.0),
        run_partition_scenario(10, 30.0),
        run_partition_scenario(50, 0.0),
        run_partition_scenario(50, 20.0),
    ];

    // Print results table
    println!("\n{:<50} {:>8} {:>8} {:>10} {:>12}",
        "Scenario", "Conv?", "Rounds", "Time", "Transferred");
    println!("{}", "-".repeat(92));

    for r in &results {
        println!("{:<50} {:>8} {:>8} {:>9.3}ms {:>12}",
            r.scenario,
            if r.converged { "YES" } else { "NO" },
            r.rounds,
            r.elapsed.as_secs_f64() * 1000.0,
            r.nodes_transferred,
        );
    }

    println!("\n{:<50} {:>8} {:>8} {:>10} {:>12}",
        "Partition Scenario", "Conv?", "Rounds", "Merge Time", "Transferred");
    println!("{}", "-".repeat(92));

    for r in &partition_results {
        println!("{:<50} {:>8} {:>8} {:>9.3}ms {:>12}",
            r.scenario,
            if r.converged { "YES" } else { "NO" },
            r.rounds,
            r.elapsed.as_secs_f64() * 1000.0,
            r.nodes_transferred,
        );
    }

    // Convergence under extreme conditions
    println!("\n--- Stress Test: Convergence threshold ---");
    for loss in [60.0, 70.0, 80.0, 90.0, 95.0] {
        let r = run_scenario(
            &format!("{:.0}% packet loss (stress)", loss),
            loss,
            0,
            0,
            500,
        );
        println!("  {:<40} conv={:<5} rounds={:<4} time={:.3}ms",
            r.scenario,
            if r.converged { "YES" } else { "NO" },
            r.rounds,
            r.elapsed.as_secs_f64() * 1000.0,
        );
    }

    println!("\n=====================================================");
    println!("BENCHMARK COMPLETE");
    println!("=====================================================");
}
