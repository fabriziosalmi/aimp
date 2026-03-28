///! Batch Signing — Multi-Node Scalability Benchmark
///!
///! Tests batch signing with 3, 5, 10, and 20 nodes to verify that
///! throughput and convergence scale correctly under batch signing mode.
///!
///! Run: RUSTFLAGS="-C target-cpu=native" cargo run --release \
///!        --features fast-crypto --example bench_batch_multinode
use aimp_node::crdt::merkle_dag::{DagNode, MerkleCrdtEngine};
use aimp_node::crypto::{Identity, SecurityFirewall};
use smallvec::SmallVec;
use std::collections::BTreeMap;
use std::time::Instant;

fn compute_batch_root(hashes: &[[u8; 32]]) -> [u8; 32] {
    if hashes.is_empty() {
        return [0u8; 32];
    }
    if hashes.len() == 1 {
        return hashes[0];
    }

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

fn run_scenario(num_nodes: usize, mutations_per_node: usize, batch_size: usize) {
    let mut engines: Vec<MerkleCrdtEngine> = (0..num_nodes)
        .map(|_| MerkleCrdtEngine::with_gc_threshold(None, 100_000))
        .collect();
    let identities: Vec<Identity> = (0..num_nodes).map(|_| Identity::new()).collect();

    // Phase 1: Each node creates mutations with batch signing
    let mutation_start = Instant::now();

    for (node_idx, engine) in engines.iter_mut().enumerate() {
        let identity = &identities[node_idx];
        let mut pending_hashes: Vec<[u8; 32]> = Vec::with_capacity(batch_size);

        for tick in 0..mutations_per_node {
            let data = format!("n{}-m{}", node_idx, tick);
            let data_hash = SecurityFirewall::hash(data.as_bytes());

            // Accumulate hash
            pending_hashes.push(data_hash);

            let sig = if pending_hashes.len() >= batch_size {
                // Batch full: compute root and sign
                let root = compute_batch_root(&pending_hashes);
                let s = identity.sign_bytes(&root);
                pending_hashes.clear();
                s
            } else {
                [0u8; 64] // Placeholder until batch closes
            };

            let mut vc = BTreeMap::new();
            vc.insert(format!("n{}", node_idx), tick as u64 + 1);
            engine.append_mutation(data_hash, sig, vc, None);
        }

        // Close final partial batch
        if !pending_hashes.is_empty() {
            let _root = compute_batch_root(&pending_hashes);
            let _ = identity.sign_bytes(&_root);
        }
    }

    let mutation_elapsed = mutation_start.elapsed();
    let total_mutations = num_nodes * mutations_per_node;
    let mutation_rate = total_mutations as f64 / mutation_elapsed.as_secs_f64();

    // Phase 2: Full mesh sync (anti-entropy)
    let sync_start = Instant::now();
    let mut total_transferred = 0;
    let mut rounds = 0;

    loop {
        rounds += 1;
        let mut transferred = 0;

        for i in 0..num_nodes {
            for j in 0..num_nodes {
                if i == j {
                    continue;
                }
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

        total_transferred += transferred;

        let roots: Vec<_> = engines.iter_mut().map(|e| e.get_merkle_root()).collect();
        let distinct = roots.iter().collect::<std::collections::HashSet<_>>().len();

        if distinct == 1 || transferred == 0 || rounds > 10 {
            let sync_elapsed = sync_start.elapsed();
            let converged = distinct == 1;

            println!(
                "  {:>2} nodes x {:>4} mut, batch={:<3} | {:>8.0} mut/s | sync {:.3}ms ({} rounds) | conv={} | DAG={}",
                num_nodes, mutations_per_node, batch_size,
                mutation_rate,
                sync_elapsed.as_secs_f64() * 1000.0,
                rounds,
                if converged { "YES" } else { "NO " },
                engines[0].arena.len()
            );
            break;
        }
    }
}

fn main() {
    println!("AIMP Batch Signing — Multi-Node Scalability");
    println!("============================================\n");
    println!(
        "  {:>2} {:>6} {:>6} | {:>10} | {:>12} {:>8} | {:>4} | {:>5}",
        "N", "mut/n", "batch", "mut/s", "sync", "rounds", "conv", "DAG"
    );
    println!("{}", "-".repeat(80));

    // Vary node count
    for num_nodes in [3, 5, 10, 20] {
        for batch_size in [1, 10, 20, 50] {
            run_scenario(num_nodes, 100, batch_size);
        }
        println!();
    }

    // High mutation count with batch signing
    println!("--- High throughput (1000 mutations/node) ---\n");
    for num_nodes in [3, 5, 10] {
        for batch_size in [10, 20, 50] {
            run_scenario(num_nodes, 1000, batch_size);
        }
        println!();
    }

    println!("============================================");
    println!("BENCHMARK COMPLETE");
}
