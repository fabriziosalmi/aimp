///! AIMP Stress Test — Find the breaking point
///!
///! Scales nodes (3→100), mutations (100→10000), batch sizes (1→500),
///! and DAG depth to find where performance degrades and convergence breaks.
///!
///! Run: RUSTFLAGS="-C target-cpu=native" cargo run --release \
///!        --features fast-crypto --example bench_stress

use aimp_node::crdt::merkle_dag::{DagNode, MerkleCrdtEngine};
use aimp_node::crypto::{Identity, SecurityFirewall};
use std::collections::BTreeMap;
use std::time::Instant;

fn compute_batch_root(hashes: &[[u8; 32]]) -> [u8; 32] {
    if hashes.is_empty() { return [0u8; 32]; }
    if hashes.len() == 1 { return hashes[0]; }
    let mut level: Vec<[u8; 32]> = hashes.to_vec();
    while level.len() > 1 {
        let mut next = Vec::with_capacity((level.len() + 1) / 2);
        for pair in level.chunks(2) {
            if pair.len() == 2 {
                let mut h = blake3::Hasher::new();
                h.update(&pair[0]);
                h.update(&pair[1]);
                next.push(*h.finalize().as_bytes());
            } else {
                next.push(pair[0]);
            }
        }
        level = next;
    }
    level[0]
}

fn recompute_heads(engine: &mut MerkleCrdtEngine) {
    let mut has_children = rustc_hash::FxHashSet::default();
    for (_, node) in engine.arena.get_all_iter() {
        for p in &node.parents { has_children.insert(*p); }
    }
    engine.heads.clear();
    for (hash, _) in engine.arena.get_all_iter() {
        if !has_children.contains(hash) { engine.heads.insert(*hash); }
    }
    engine.invalidate_root();
}

struct StressResult {
    num_nodes: usize,
    mutations_per_node: usize,
    batch_size: usize,
    total_mutations: usize,
    mutation_rate: f64,
    sync_time_ms: f64,
    sync_rounds: usize,
    converged: bool,
    dag_size: usize,
    heads_count: usize,
    memory_estimate_kb: usize,
}

fn run_stress(num_nodes: usize, mutations_per_node: usize, batch_size: usize) -> StressResult {
    let mut engines: Vec<MerkleCrdtEngine> = (0..num_nodes)
        .map(|_| MerkleCrdtEngine::with_gc_threshold(None, 100_000))
        .collect();
    let identities: Vec<Identity> = (0..num_nodes).map(|_| Identity::new()).collect();

    // Phase 1: Mutations with batch signing
    let mutation_start = Instant::now();

    for (ni, engine) in engines.iter_mut().enumerate() {
        let identity = &identities[ni];
        let mut pending: Vec<[u8; 32]> = Vec::with_capacity(batch_size);

        for tick in 0..mutations_per_node {
            let data_hash = SecurityFirewall::hash(
                &[ni as u8, (tick >> 8) as u8, tick as u8]
            );

            pending.push(data_hash);
            let sig = if pending.len() >= batch_size {
                let root = compute_batch_root(&pending);
                let s = identity.sign_bytes(&root);
                pending.clear();
                s
            } else {
                [0u8; 64]
            };

            let mut vc = BTreeMap::new();
            vc.insert(format!("n{}", ni), tick as u64 + 1);
            engine.append_mutation(data_hash, sig, vc, None);
        }

        if !pending.is_empty() {
            let root = compute_batch_root(&pending);
            let _ = identity.sign_bytes(&root);
        }
    }

    let mutation_elapsed = mutation_start.elapsed();
    let total_mutations = num_nodes * mutations_per_node;
    let mutation_rate = total_mutations as f64 / mutation_elapsed.as_secs_f64();

    // Phase 2: Full mesh sync
    let sync_start = Instant::now();
    let mut rounds = 0;
    let converged;

    loop {
        rounds += 1;
        let mut transferred = 0;

        for i in 0..num_nodes {
            for j in 0..num_nodes {
                if i == j { continue; }
                let src_nodes: Vec<([u8; 32], DagNode)> = engines[i]
                    .arena.get_all_iter()
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
                if added { recompute_heads(dst); }
            }
        }

        let roots: Vec<_> = engines.iter_mut().map(|e| e.get_merkle_root()).collect();
        let distinct = roots.iter().collect::<std::collections::HashSet<_>>().len();

        if distinct == 1 {
            converged = true;
            break;
        }
        if transferred == 0 || rounds > 10 {
            converged = distinct == 1;
            break;
        }
    }

    let sync_elapsed = sync_start.elapsed();
    let dag_size = engines[0].arena.len();
    let heads_count = engines[0].heads.len();
    // Rough memory estimate: each DagNode ~ 200 bytes
    let memory_estimate_kb = dag_size * 200 / 1024;

    StressResult {
        num_nodes,
        mutations_per_node,
        batch_size,
        total_mutations,
        mutation_rate,
        sync_time_ms: sync_elapsed.as_secs_f64() * 1000.0,
        sync_rounds: rounds,
        converged,
        dag_size,
        heads_count,
        memory_estimate_kb,
    }
}

fn print_result(r: &StressResult) {
    println!(
        "  {:>3}N x {:>5}M  b={:<4} | {:>8.0} mut/s | {:>10} total | sync {:>9.1}ms {:>2}R | {} | DAG {:>6} heads {:>3} ~{:>5}KB",
        r.num_nodes, r.mutations_per_node, r.batch_size,
        r.mutation_rate,
        r.total_mutations,
        r.sync_time_ms, r.sync_rounds,
        if r.converged { "OK " } else { "FAIL" },
        r.dag_size, r.heads_count,
        r.memory_estimate_kb
    );
}

fn main() {
    println!("AIMP Stress Test — Finding the Breaking Point");
    println!("==============================================\n");

    // ---- Scale: Node count ----
    println!("--- Scale: Node Count (100 mut/node, batch=20) ---\n");
    for n in [3, 5, 10, 20, 30, 50, 75, 100] {
        let r = run_stress(n, 100, 20);
        print_result(&r);
    }

    // ---- Scale: Mutations per node ----
    println!("\n--- Scale: Mutations/Node (5 nodes, batch=20) ---\n");
    for m in [100, 500, 1000, 2000, 5000, 10000] {
        let r = run_stress(5, m, 20);
        print_result(&r);
    }

    // ---- Scale: Batch size ----
    println!("\n--- Scale: Batch Size (10 nodes, 500 mut/node) ---\n");
    for b in [1, 5, 10, 20, 50, 100, 200, 500] {
        let r = run_stress(10, 500, b);
        print_result(&r);
    }

    // ---- Extreme: Large clusters ----
    println!("\n--- Extreme: Large DAGs (batch=50) ---\n");
    for (n, m) in [(5, 10000), (10, 5000), (20, 2000), (50, 1000), (100, 500)] {
        let r = run_stress(n, m, 50);
        print_result(&r);
    }

    // ---- Extreme: Memory pressure ----
    println!("\n--- Extreme: Memory Pressure (10 nodes, batch=50) ---\n");
    for m in [1000, 5000, 10000, 20000] {
        let r = run_stress(10, m, 50);
        print_result(&r);
        if r.memory_estimate_kb > 500_000 {
            println!("    (stopped: estimated memory > 500MB)");
            break;
        }
    }

    println!("\n==============================================");
    println!("STRESS TEST COMPLETE");
}
