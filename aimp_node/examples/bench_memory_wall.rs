///! Memory Wall Benchmark — L3 Cache Exhaustion Detection
///!
///! Pumps millions of mutations into a single engine with GC disabled,
///! tracking throughput every 500K ops. When the working set exceeds
///! L3 cache (~32MB on Apple Silicon), throughput drops as every
///! FxHashMap/arena lookup becomes a cache miss (~50-100ns penalty).
///!
///! Run: RUSTFLAGS="-C target-cpu=native" cargo run --release \
///!        --features fast-crypto --example bench_memory_wall

use aimp_node::crdt::merkle_dag::MerkleCrdtEngine;
use aimp_node::crypto::{Identity, SecurityFirewall};
use std::collections::BTreeMap;
use std::time::Instant;

const REPORT_INTERVAL: usize = 500_000;
const MAX_MUTATIONS: usize = 20_000_000;

fn main() {
    println!("AIMP Memory Wall Benchmark — L3 Cache Exhaustion");
    println!("=================================================");
    println!("GC disabled. Tracking throughput as working set grows.\n");

    let identity = Identity::new();
    // GC threshold set impossibly high to disable it
    let mut engine = MerkleCrdtEngine::with_gc_threshold(None, u64::MAX);

    let mut batch_hashes: Vec<[u8; 32]> = Vec::with_capacity(50);
    let batch_size = 50;

    println!(
        "{:>8} {:>10} {:>12} {:>12} {:>10}",
        "Muts", "DAG size", "~Memory", "Interval", "Cumul"
    );
    println!(
        "{:>8} {:>10} {:>12} {:>12} {:>10}",
        "", "", "", "ops/sec", "ops/sec"
    );
    println!("{}", "-".repeat(60));

    let global_start = Instant::now();
    let mut interval_start = Instant::now();
    let mut interval_ops = 0usize;

    for i in 0..MAX_MUTATIONS {
        // Minimal mutation: 3-byte data, batch signing
        let data_hash = SecurityFirewall::hash(&[
            (i >> 16) as u8,
            (i >> 8) as u8,
            i as u8,
        ]);

        batch_hashes.push(data_hash);
        let sig = if batch_hashes.len() >= batch_size {
            // Compute batch root
            let mut level = batch_hashes.clone();
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
            let root = level[0];
            let s = identity.sign_bytes(&root);
            batch_hashes.clear();
            s
        } else {
            [0u8; 64]
        };

        let mut vc = BTreeMap::new();
        vc.insert("n0".to_string(), i as u64);
        engine.append_mutation(data_hash, sig, vc, None);

        interval_ops += 1;

        if interval_ops >= REPORT_INTERVAL {
            let interval_elapsed = interval_start.elapsed();
            let interval_rate = interval_ops as f64 / interval_elapsed.as_secs_f64();
            let global_elapsed = global_start.elapsed();
            let global_rate = (i + 1) as f64 / global_elapsed.as_secs_f64();
            let dag_size = engine.arena.len();
            let memory_kb = dag_size * 200 / 1024;
            let memory_str = if memory_kb > 1024 {
                format!("{:.1} MB", memory_kb as f64 / 1024.0)
            } else {
                format!("{} KB", memory_kb)
            };

            println!(
                "{:>7}K {:>10} {:>12} {:>11.0}K {:>9.0}K",
                (i + 1) / 1000,
                dag_size,
                memory_str,
                interval_rate / 1000.0,
                global_rate / 1000.0,
            );

            interval_start = Instant::now();
            interval_ops = 0;

            // Stop if throughput drops below 100K or memory exceeds 2GB
            if memory_kb > 2_000_000 {
                println!("\n  Stopped: memory estimate > 2GB");
                break;
            }
        }
    }

    let total_elapsed = global_start.elapsed();
    let dag_size = engine.arena.len();
    let total_rate = dag_size as f64 / total_elapsed.as_secs_f64();

    println!("\n=================================================");
    println!("Final: {} DAG nodes, {:.1}s, {:.0}K ops/sec avg",
        dag_size,
        total_elapsed.as_secs_f64(),
        total_rate / 1000.0,
    );
    println!("Memory: ~{:.1} MB ({} nodes × ~200 bytes)",
        dag_size as f64 * 200.0 / 1024.0 / 1024.0,
        dag_size);
    println!("=================================================");
}
