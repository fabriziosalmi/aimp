///! Mutation hot-path profiler — measures time spent in each step
///!
///! Run: cargo run --release --example profile_mutation
use aimp_node::crdt::merkle_dag::MerkleCrdtEngine;
use aimp_node::crypto::{Identity, SecurityFirewall};
use aimp_node::protocol::{AimpData, OpCode};
use std::collections::BTreeMap;
use std::time::Instant;

const ITERATIONS: usize = 10_000;

fn main() {
    println!("AIMP Mutation Hot-Path Profiler ({ITERATIONS} iterations)");
    println!("=========================================================\n");

    let identity = Identity::new();
    let mut engine = MerkleCrdtEngine::with_gc_threshold(None, 100_000); // disable GC

    // Warm up
    for i in 0..100 {
        let data = format!("warmup-{}", i);
        let data_hash = SecurityFirewall::hash(data.as_bytes());
        let aimp_data = AimpData {
            v: 1,
            op: OpCode::Ping,
            ttl: 3,
            origin_pubkey: identity.node_id(),
            vclock: BTreeMap::new(),
            payload: data.into_bytes(),
        };
        let bytes = rmp_serde::to_vec(&aimp_data).unwrap();
        let sig = identity.sign_bytes(&bytes);
        let mut vc = BTreeMap::new();
        vc.insert("n0".to_string(), i as u64);
        engine.append_mutation(data_hash, sig, vc, None);
    }

    // Reset engine for clean measurement
    engine = MerkleCrdtEngine::with_gc_threshold(None, 100_000);

    let mut t_format = 0u64;
    let mut t_blake3 = 0u64;
    let mut t_serialize = 0u64;
    let mut t_sign = 0u64;
    let mut t_vclock = 0u64;
    let mut t_append = 0u64;
    let mut t_total = 0u64;

    for i in 0..ITERATIONS {
        let total_start = Instant::now();

        // Step 1: Format data
        let s1 = Instant::now();
        let data = format!("mutation-{}", i);
        t_format += s1.elapsed().as_nanos() as u64;

        // Step 2: BLAKE3 hash
        let s2 = Instant::now();
        let data_hash = SecurityFirewall::hash(data.as_bytes());
        t_blake3 += s2.elapsed().as_nanos() as u64;

        // Step 3: Serialize AimpData
        let s3 = Instant::now();
        let aimp_data = AimpData {
            v: 1,
            op: OpCode::Ping,
            ttl: 3,
            origin_pubkey: identity.node_id(),
            vclock: BTreeMap::new(),
            payload: data.into_bytes(),
        };
        let bytes = rmp_serde::to_vec(&aimp_data).unwrap();
        t_serialize += s3.elapsed().as_nanos() as u64;

        // Step 4: Ed25519 sign
        let s4 = Instant::now();
        let sig = identity.sign_bytes(&bytes);
        t_sign += s4.elapsed().as_nanos() as u64;

        // Step 5: VClock creation
        let s5 = Instant::now();
        let mut vc = BTreeMap::new();
        vc.insert("n0".to_string(), i as u64);
        t_vclock += s5.elapsed().as_nanos() as u64;

        // Step 6: append_mutation (hash + arena insert + heads update)
        let s6 = Instant::now();
        engine.append_mutation(data_hash, sig, vc, None);
        t_append += s6.elapsed().as_nanos() as u64;

        t_total += total_start.elapsed().as_nanos() as u64;
    }

    let n = ITERATIONS as f64;

    println!("Per-mutation breakdown (average over {ITERATIONS} ops):\n");
    println!(
        "  {:20} {:>8.1} ns  ({:>5.1}%)",
        "format!()",
        t_format as f64 / n,
        t_format as f64 / t_total as f64 * 100.0
    );
    println!(
        "  {:20} {:>8.1} ns  ({:>5.1}%)",
        "BLAKE3 hash",
        t_blake3 as f64 / n,
        t_blake3 as f64 / t_total as f64 * 100.0
    );
    println!(
        "  {:20} {:>8.1} ns  ({:>5.1}%)",
        "rmp_serde serialize",
        t_serialize as f64 / n,
        t_serialize as f64 / t_total as f64 * 100.0
    );
    println!(
        "  {:20} {:>8.1} ns  ({:>5.1}%)",
        "Ed25519 sign",
        t_sign as f64 / n,
        t_sign as f64 / t_total as f64 * 100.0
    );
    println!(
        "  {:20} {:>8.1} ns  ({:>5.1}%)",
        "BTreeMap vclock",
        t_vclock as f64 / n,
        t_vclock as f64 / t_total as f64 * 100.0
    );
    println!(
        "  {:20} {:>8.1} ns  ({:>5.1}%)",
        "append_mutation()",
        t_append as f64 / n,
        t_append as f64 / t_total as f64 * 100.0
    );
    println!("  {:20} {:>8.1} ns", "─────────────────", 0.0);
    println!("  {:20} {:>8.1} ns  (100%)", "TOTAL", t_total as f64 / n);

    let overhead = t_total as f64 - t_sign as f64;
    println!(
        "\n  Ed25519 sign:     {:.1} µs ({:.1}%)",
        t_sign as f64 / n / 1000.0,
        t_sign as f64 / t_total as f64 * 100.0
    );
    println!(
        "  Non-crypto total: {:.1} µs ({:.1}%)",
        overhead / n / 1000.0,
        overhead / t_total as f64 * 100.0
    );

    println!(
        "\n  Theoretical max (sign only): {:.0} ops/sec",
        1_000_000_000.0 / (t_sign as f64 / n)
    );
    println!(
        "  Actual throughput:           {:.0} ops/sec",
        1_000_000_000.0 / (t_total as f64 / n)
    );
    println!(
        "  Efficiency:                  {:.1}%",
        (t_sign as f64 / t_total as f64) * 100.0
    );
}
