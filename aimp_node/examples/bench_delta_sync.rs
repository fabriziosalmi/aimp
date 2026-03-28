///! Delta-Sync Benchmark — O(Δ) vs O(D) anti-entropy
///!
///! Compares three sync strategies:
///!   1. Full-state: copy entire arena (current stress test) — O(N²×D)
///!   2. Delta-vdiff: exchange only missing nodes via get_vdiff — O(N²×Δ)
///!   3. Gossip fan-out: delta-vdiff with K random peers/round — O(N×K×Δ)
///!
///! Run: RUSTFLAGS="-C target-cpu=native" cargo run --release \
///!        --features fast-crypto --example bench_delta_sync
use aimp_node::crdt::merkle_dag::{DagNode, MerkleCrdtEngine};
use aimp_node::crypto::{Identity, SecurityFirewall};
use rand::prelude::*;
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

fn create_engines(
    num_nodes: usize,
    mutations_per_node: usize,
    batch_size: usize,
) -> Vec<MerkleCrdtEngine> {
    let mut engines: Vec<MerkleCrdtEngine> = (0..num_nodes)
        .map(|_| MerkleCrdtEngine::with_gc_threshold(None, 100_000))
        .collect();
    let identities: Vec<Identity> = (0..num_nodes).map(|_| Identity::new()).collect();

    for (ni, engine) in engines.iter_mut().enumerate() {
        let identity = &identities[ni];
        let mut pending: Vec<[u8; 32]> = Vec::with_capacity(batch_size);
        for tick in 0..mutations_per_node {
            let data_hash = SecurityFirewall::hash(&[ni as u8, (tick >> 8) as u8, tick as u8]);
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
    }
    engines
}

fn check_convergence(engines: &mut [MerkleCrdtEngine]) -> (bool, usize) {
    let roots: Vec<_> = engines.iter_mut().map(|e| e.get_merkle_root()).collect();
    let distinct = roots.iter().collect::<std::collections::HashSet<_>>().len();
    (distinct == 1, distinct)
}

/// Strategy 1: Full-state transfer (O(N²×D))
fn sync_full_state(engines: &mut Vec<MerkleCrdtEngine>) -> (f64, usize) {
    let n = engines.len();
    let start = Instant::now();
    let mut rounds = 0;

    loop {
        rounds += 1;
        let mut transferred = 0;
        for i in 0..n {
            for j in 0..n {
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
        let (conv, _) = check_convergence(engines);
        if conv || transferred == 0 || rounds > 20 {
            break;
        }
    }
    (start.elapsed().as_secs_f64() * 1000.0, rounds)
}

/// Strategy 2: Delta-vdiff (O(N²×Δ))
fn sync_delta_vdiff(engines: &mut Vec<MerkleCrdtEngine>) -> (f64, usize, usize) {
    let n = engines.len();
    let start = Instant::now();
    let mut rounds = 0;
    let mut total_transferred = 0;

    loop {
        rounds += 1;
        let mut transferred = 0;

        for i in 0..n {
            for j in 0..n {
                if i == j {
                    continue;
                }
                // Get remote heads
                let remote_heads: Vec<_> = engines[j].heads.iter().copied().collect();
                // Compute delta: only nodes j doesn't have
                let delta = engines[i].get_vdiff(remote_heads);

                if !delta.is_empty() {
                    let dst = &mut engines[j];
                    for node in &delta {
                        let hash = node.compute_hash();
                        if !dst.arena.contains(&hash) {
                            dst.arena.insert(hash, node.clone());
                            transferred += 1;
                        }
                    }
                    recompute_heads(dst);
                }
            }
        }

        total_transferred += transferred;
        let (conv, _) = check_convergence(engines);
        if conv || transferred == 0 || rounds > 20 {
            break;
        }
    }
    (
        start.elapsed().as_secs_f64() * 1000.0,
        rounds,
        total_transferred,
    )
}

/// Strategy 3: Gossip fan-out with delta-vdiff (O(N×K×Δ))
fn sync_gossip_fanout(engines: &mut Vec<MerkleCrdtEngine>, fanout: usize) -> (f64, usize, usize) {
    let n = engines.len();
    let mut rng = rand::thread_rng();
    let start = Instant::now();
    let mut rounds = 0;
    let mut total_transferred = 0;

    loop {
        rounds += 1;
        let mut transferred = 0;

        for i in 0..n {
            // Pick K random peers (gossip fan-out)
            let mut peers: Vec<usize> = (0..n).filter(|&j| j != i).collect();
            peers.shuffle(&mut rng);
            peers.truncate(fanout);

            for &j in &peers {
                let remote_heads: Vec<_> = engines[j].heads.iter().copied().collect();
                let delta = engines[i].get_vdiff(remote_heads);

                if !delta.is_empty() {
                    let dst = &mut engines[j];
                    for node in &delta {
                        let hash = node.compute_hash();
                        if !dst.arena.contains(&hash) {
                            dst.arena.insert(hash, node.clone());
                            transferred += 1;
                        }
                    }
                    recompute_heads(dst);
                }
            }
        }

        total_transferred += transferred;
        let (conv, _) = check_convergence(engines);
        if conv || transferred == 0 || rounds > 50 {
            break;
        }
    }
    (
        start.elapsed().as_secs_f64() * 1000.0,
        rounds,
        total_transferred,
    )
}

fn main() {
    println!("AIMP Delta-Sync Benchmark");
    println!("=========================\n");

    let batch_size = 50;

    println!(
        "{:>5} {:>6} {:>12} {:>12} {:>12} {:>8} {:>8} {:>8}",
        "Nodes", "Mut/N", "Full-State", "Delta-Vdiff", "Gossip(3)", "R:Full", "R:Delta", "R:Goss"
    );
    println!("{}", "-".repeat(85));

    for (num_nodes, mutations_per_node) in [
        (5, 100),
        (10, 100),
        (20, 100),
        (30, 100),
        (50, 100),
        (5, 500),
        (10, 500),
        (20, 500),
        (5, 1000),
        (10, 1000),
        (50, 500),
        (100, 100),
    ] {
        // Create 3 identical copies of the same initial state
        let mut engines_full = create_engines(num_nodes, mutations_per_node, batch_size);
        let mut engines_delta = create_engines(num_nodes, mutations_per_node, batch_size);
        let mut engines_gossip = create_engines(num_nodes, mutations_per_node, batch_size);

        let (full_ms, full_rounds) = sync_full_state(&mut engines_full);
        let (delta_ms, delta_rounds, _delta_nodes) = sync_delta_vdiff(&mut engines_delta);

        let fanout = 3.min(num_nodes - 1);
        let (gossip_ms, gossip_rounds, _gossip_nodes) =
            sync_gossip_fanout(&mut engines_gossip, fanout);

        // Verify all converged
        let (fc, _) = check_convergence(&mut engines_full);
        let (dc, _) = check_convergence(&mut engines_delta);
        let (gc, _) = check_convergence(&mut engines_gossip);

        let full_str = if fc {
            format!("{:.1}ms", full_ms)
        } else {
            "FAIL".to_string()
        };
        let delta_str = if dc {
            format!("{:.1}ms", delta_ms)
        } else {
            "FAIL".to_string()
        };
        let gossip_str = if gc {
            format!("{:.1}ms", gossip_ms)
        } else {
            "FAIL".to_string()
        };

        println!(
            "{:>5} {:>6} {:>12} {:>12} {:>12} {:>8} {:>8} {:>8}",
            num_nodes,
            mutations_per_node,
            full_str,
            delta_str,
            gossip_str,
            full_rounds,
            delta_rounds,
            gossip_rounds,
        );
    }

    println!("\n=========================");
    println!("Legend: Full-State=O(N²×D), Delta-Vdiff=O(N²×Δ), Gossip(K)=O(N×K×Δ)");
    println!("All times are wall-clock for full convergence (all nodes same root).");
}
