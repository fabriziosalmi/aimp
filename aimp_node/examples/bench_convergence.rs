///! System-level convergence benchmark for AIMP
///!
///! Simulates N nodes communicating in-process via channels.
///! Measures:
///!   1. Mutation throughput (end-to-end, N nodes)
///!   2. Convergence time after asymmetric mutations
///!   3. Partition/merge convergence time
///!
///! Run: cargo run --release --manifest-path aimp_node/Cargo.toml --example bench_convergence
use aimp_node::crdt::merkle_dag::{DagNode, MerkleCrdtEngine};
use aimp_node::crypto::{Identity, SecurityFirewall};
use aimp_node::protocol::{AimpData, OpCode};
use std::collections::BTreeMap;
use std::time::Instant;

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

/// Full-state transfer: copy ALL nodes from src that dst doesn't have.
/// This simulates anti-entropy sync (like real CRDT replication).
fn full_sync(src: &MerkleCrdtEngine, dst: &mut MerkleCrdtEngine) -> usize {
    let mut added = 0;
    let all_nodes: Vec<([u8; 32], DagNode)> = src
        .arena
        .get_all_iter()
        .map(|(h, n)| (*h, n.clone()))
        .collect();

    for (hash, node) in all_nodes {
        if !dst.arena.contains(&hash) {
            dst.arena.insert(hash, node);
            added += 1;
        }
    }

    if added > 0 {
        // Recompute heads: a node is a head if no other node lists it as parent
        let mut has_children = std::collections::HashSet::new();
        for (_, node) in dst.arena.get_all_iter() {
            for p in &node.parents {
                has_children.insert(*p);
            }
        }
        dst.heads.clear();
        for (hash, _) in dst.arena.get_all_iter() {
            if !has_children.contains(hash) {
                dst.heads.insert(*hash);
            }
        }
        dst.invalidate_root();
    }
    added
}

fn print_separator(title: &str) {
    println!("\n--- {title} ---");
}

fn main() {
    println!("AIMP System-Level Convergence Benchmark");
    println!("========================================\n");

    let num_nodes: usize = 5;

    // -----------------------------------------------------------------------
    // Benchmark 1: Mutation Throughput
    // -----------------------------------------------------------------------
    {
        let mutations_per_node: usize = 1000;
        print_separator(&format!(
            "Benchmark 1: Throughput ({num_nodes} nodes x {mutations_per_node} mutations)"
        ));

        let mut engines: Vec<MerkleCrdtEngine> = (0..num_nodes)
            .map(|_| MerkleCrdtEngine::default())
            .collect();
        let identities: Vec<Identity> = (0..num_nodes).map(|_| Identity::new()).collect();

        let start = Instant::now();
        for (i, engine) in engines.iter_mut().enumerate() {
            for tick in 0..mutations_per_node {
                let data = format!("n{}-m{}", i, tick);
                create_mutation(
                    engine,
                    &identities[i],
                    data.as_bytes(),
                    &format!("n{i}"),
                    tick + 1,
                );
            }
        }
        let elapsed = start.elapsed();
        let total = num_nodes * mutations_per_node;
        let rate = total as f64 / elapsed.as_secs_f64();

        println!("  Total mutations: {total}");
        println!("  Time: {:.3}ms", elapsed.as_secs_f64() * 1000.0);
        println!("  Rate: {rate:.0} mutations/sec (with Ed25519 sign per mutation)");
    }

    // -----------------------------------------------------------------------
    // Benchmark 2: Convergence (anti-entropy full sync)
    // -----------------------------------------------------------------------
    {
        let mutations_per_node: usize = 50;
        print_separator(&format!(
            "Benchmark 2: Convergence ({num_nodes} nodes x {mutations_per_node} mutations, anti-entropy sync)"
        ));

        let mut engines: Vec<MerkleCrdtEngine> = (0..num_nodes)
            .map(|_| MerkleCrdtEngine::default())
            .collect();
        let identities: Vec<Identity> = (0..num_nodes).map(|_| Identity::new()).collect();

        // Each node creates independent mutations
        for (i, engine) in engines.iter_mut().enumerate() {
            for tick in 0..mutations_per_node {
                let data = format!("conv-n{}-m{}", i, tick);
                create_mutation(
                    engine,
                    &identities[i],
                    data.as_bytes(),
                    &format!("n{i}"),
                    tick + 1,
                );
            }
        }

        let roots_before: Vec<_> = engines.iter_mut().map(|e| e.get_merkle_root()).collect();
        let distinct_before = roots_before
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len();
        println!("  Distinct roots before sync: {distinct_before}");

        // Full-mesh anti-entropy sync
        let sync_start = Instant::now();
        let mut rounds = 0;
        let mut total_nodes_transferred = 0;

        loop {
            rounds += 1;
            let mut transferred_this_round = 0;

            for i in 0..num_nodes {
                for j in 0..num_nodes {
                    if i == j {
                        continue;
                    }
                    // Clone source data to satisfy borrow checker
                    let src_nodes: Vec<([u8; 32], DagNode)> = engines[i]
                        .arena
                        .get_all_iter()
                        .map(|(h, n)| (*h, n.clone()))
                        .collect();

                    let dst = &mut engines[j];
                    let mut added = 0;
                    for (hash, node) in &src_nodes {
                        if !dst.arena.contains(hash) {
                            dst.arena.insert(*hash, node.clone());
                            added += 1;
                        }
                    }
                    if added > 0 {
                        // Recompute heads
                        let mut has_children = std::collections::HashSet::new();
                        for (_, node) in dst.arena.get_all_iter() {
                            for p in &node.parents {
                                has_children.insert(*p);
                            }
                        }
                        dst.heads.clear();
                        for (hash, _) in dst.arena.get_all_iter() {
                            if !has_children.contains(hash) {
                                dst.heads.insert(*hash);
                            }
                        }
                        dst.invalidate_root();
                        transferred_this_round += added;
                    }
                }
            }

            total_nodes_transferred += transferred_this_round;

            let roots: Vec<_> = engines.iter_mut().map(|e| e.get_merkle_root()).collect();
            let distinct = roots.iter().collect::<std::collections::HashSet<_>>().len();

            if distinct == 1 {
                let elapsed = sync_start.elapsed();
                println!(
                    "  CONVERGED in {:.3}ms ({rounds} rounds)",
                    elapsed.as_secs_f64() * 1000.0
                );
                println!("  Nodes transferred: {total_nodes_transferred}");
                println!("  DAG nodes per engine: {}", engines[0].arena.len());
                println!("  Heads per engine: {}", engines[0].heads.len());
                println!("  Final root: {}", hex::encode(roots[0]));
                break;
            }

            if transferred_this_round == 0 || rounds > 10 {
                println!("  STALLED after {rounds} rounds ({distinct} distinct roots, {transferred_this_round} transferred last round)");
                for (i, root) in roots.iter().enumerate() {
                    println!(
                        "    node{i}: {} (arena={}, heads={})",
                        hex::encode(root),
                        engines[i].arena.len(),
                        engines[i].heads.len()
                    );
                }
                break;
            }
        }
    }

    // -----------------------------------------------------------------------
    // Benchmark 3: Partition + Merge
    // -----------------------------------------------------------------------
    {
        let mutations_per_group = 30;
        print_separator(&format!(
            "Benchmark 3: Partition/Merge ({num_nodes} nodes, {mutations_per_group} mutations per group)"
        ));

        // Start with converged state
        let mut engines: Vec<MerkleCrdtEngine> = (0..num_nodes)
            .map(|_| MerkleCrdtEngine::default())
            .collect();
        let identities: Vec<Identity> = (0..num_nodes).map(|_| Identity::new()).collect();

        // Shared baseline: node 0 creates 10 mutations, synced to all
        for tick in 0..10usize {
            let data = format!("shared-{}", tick);
            create_mutation(
                &mut engines[0],
                &identities[0],
                data.as_bytes(),
                "n0",
                tick + 1,
            );
        }
        for j in 1..num_nodes {
            let src_nodes: Vec<_> = engines[0]
                .arena
                .get_all_iter()
                .map(|(h, n)| (*h, n.clone()))
                .collect();
            let dst = &mut engines[j];
            for (hash, node) in &src_nodes {
                if !dst.arena.contains(hash) {
                    dst.arena.insert(*hash, node.clone());
                }
            }
            let mut has_children = std::collections::HashSet::new();
            for (_, node) in dst.arena.get_all_iter() {
                for p in &node.parents {
                    has_children.insert(*p);
                }
            }
            dst.heads.clear();
            for (hash, _) in dst.arena.get_all_iter() {
                if !has_children.contains(hash) {
                    dst.heads.insert(*hash);
                }
            }
            dst.invalidate_root();
        }

        // Verify baseline convergence
        let roots: Vec<_> = engines.iter_mut().map(|e| e.get_merkle_root()).collect();
        assert_eq!(
            roots.iter().collect::<std::collections::HashSet<_>>().len(),
            1
        );
        println!("  Baseline converged: {}", hex::encode(roots[0]));

        // PARTITION: Group A [0,1] and Group B [2,3,4]
        let partition_start = Instant::now();

        for tick in 0..mutations_per_group {
            let data = format!("partA-{}", tick);
            create_mutation(
                &mut engines[0],
                &identities[0],
                data.as_bytes(),
                "n0",
                100 + tick,
            );
        }
        // Sync within A
        {
            let src_nodes: Vec<_> = engines[0]
                .arena
                .get_all_iter()
                .map(|(h, n)| (*h, n.clone()))
                .collect();
            let dst = &mut engines[1];
            for (hash, node) in &src_nodes {
                if !dst.arena.contains(hash) {
                    dst.arena.insert(*hash, node.clone());
                }
            }
            let mut has_children = std::collections::HashSet::new();
            for (_, node) in dst.arena.get_all_iter() {
                for p in &node.parents {
                    has_children.insert(*p);
                }
            }
            dst.heads.clear();
            for (hash, _) in dst.arena.get_all_iter() {
                if !has_children.contains(hash) {
                    dst.heads.insert(*hash);
                }
            }
            dst.invalidate_root();
        }

        for tick in 0..mutations_per_group {
            let data = format!("partB-{}", tick);
            create_mutation(
                &mut engines[2],
                &identities[2],
                data.as_bytes(),
                "n2",
                100 + tick,
            );
        }
        // Sync within B
        for j in [3usize, 4] {
            let src_nodes: Vec<_> = engines[2]
                .arena
                .get_all_iter()
                .map(|(h, n)| (*h, n.clone()))
                .collect();
            let dst = &mut engines[j];
            for (hash, node) in &src_nodes {
                if !dst.arena.contains(hash) {
                    dst.arena.insert(*hash, node.clone());
                }
            }
            let mut has_children = std::collections::HashSet::new();
            for (_, node) in dst.arena.get_all_iter() {
                for p in &node.parents {
                    has_children.insert(*p);
                }
            }
            dst.heads.clear();
            for (hash, _) in dst.arena.get_all_iter() {
                if !has_children.contains(hash) {
                    dst.heads.insert(*hash);
                }
            }
            dst.invalidate_root();
        }

        let partition_elapsed = partition_start.elapsed();
        let roots: Vec<_> = engines.iter_mut().map(|e| e.get_merkle_root()).collect();
        let distinct = roots.iter().collect::<std::collections::HashSet<_>>().len();
        println!(
            "  Partition phase: {:.3}ms ({mutations_per_group} mutations/group)",
            partition_elapsed.as_secs_f64() * 1000.0
        );
        println!("  Distinct roots after partition: {distinct}");

        // MERGE: full mesh sync
        let merge_start = Instant::now();
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
                    for (hash, node) in &src_nodes {
                        if !dst.arena.contains(hash) {
                            dst.arena.insert(*hash, node.clone());
                            transferred += 1;
                        }
                    }
                    if transferred > 0 {
                        let mut has_children = std::collections::HashSet::new();
                        for (_, node) in dst.arena.get_all_iter() {
                            for p in &node.parents {
                                has_children.insert(*p);
                            }
                        }
                        dst.heads.clear();
                        for (hash, _) in dst.arena.get_all_iter() {
                            if !has_children.contains(hash) {
                                dst.heads.insert(*hash);
                            }
                        }
                        dst.invalidate_root();
                    }
                }
            }

            let roots: Vec<_> = engines.iter_mut().map(|e| e.get_merkle_root()).collect();
            let distinct = roots.iter().collect::<std::collections::HashSet<_>>().len();

            if distinct == 1 {
                let elapsed = merge_start.elapsed();
                println!(
                    "  MERGED in {:.3}ms ({rounds} rounds)",
                    elapsed.as_secs_f64() * 1000.0
                );
                println!("  Final root: {}", hex::encode(roots[0]));
                println!("  DAG nodes per engine: {}", engines[0].arena.len());
                break;
            }

            if transferred == 0 || rounds > 10 {
                println!("  STALLED after {rounds} rounds ({distinct} distinct roots)");
                break;
            }
        }
    }

    // -----------------------------------------------------------------------
    // Benchmark 4: Crypto hot-path breakdown
    // -----------------------------------------------------------------------
    {
        print_separator("Benchmark 4: Crypto hot-path breakdown (per-message cost)");

        let identity = Identity::new();
        let data = AimpData {
            v: 1,
            op: OpCode::Infer,
            ttl: 3,
            origin_pubkey: identity.node_id(),
            vclock: BTreeMap::new(),
            payload: b"benchmark payload data for crypto timing".to_vec(),
        };

        let iterations = 10_000;

        // Sign
        let sign_start = Instant::now();
        for _ in 0..iterations {
            let _ = identity.sign(data.clone()).unwrap();
        }
        let sign_elapsed = sign_start.elapsed();
        let sign_per = sign_elapsed.as_nanos() as f64 / iterations as f64;

        // Verify
        let envelope = identity.sign(data).unwrap();
        let verify_start = Instant::now();
        for _ in 0..iterations {
            let _ = SecurityFirewall::verify(&envelope);
        }
        let verify_elapsed = verify_start.elapsed();
        let verify_per = verify_elapsed.as_nanos() as f64 / iterations as f64;

        let total_per = sign_per + verify_per;
        let max_throughput = 1_000_000_000.0 / total_per;

        println!("  Sign:   {:.1} µs/op", sign_per / 1000.0);
        println!("  Verify: {:.1} µs/op", verify_per / 1000.0);
        println!("  Total:  {:.1} µs/msg", total_per / 1000.0);
        println!(
            "  Max theoretical throughput: {:.0} msg/sec per peer",
            max_throughput
        );
        println!(
            "  At rate_limit=50/sec: {:.3}% of crypto budget",
            50.0 / max_throughput * 100.0
        );
    }

    println!("\n========================================");
    println!("BENCHMARK COMPLETE");
    println!("========================================");
}
