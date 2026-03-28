//! L3 Hot-Path Profiling — mirrors Paper 1 Section 7.2
//!
//! Profiles the belief engine pipeline per-step to answer:
//! "Where is the bottleneck in L3?"
//!
//! Usage: cargo run --release --example profile_epistemic

use aimp_node::epistemic::*;
use std::time::Instant;

fn make_fingerprint(data: &[u8], sensor: u8) -> SemanticFingerprint {
    let hash = blake3::hash(data);
    let mut primary = [0u8; 16];
    primary.copy_from_slice(&hash.as_bytes()[..16]);
    let mut feat = blake3::Hasher::new();
    feat.update(&[sensor]);
    let secondary = u64::from_le_bytes(feat.finalize().as_bytes()[..8].try_into().unwrap());
    SemanticFingerprint { primary, secondary }
}

fn make_claim(i: u32) -> Claim {
    let mut src = [0u8; 32];
    src[..4].copy_from_slice(&i.to_le_bytes());
    let fp = make_fingerprint(&i.to_le_bytes(), 1);
    let mut h = blake3::Hasher::new();
    h.update(&fp.primary);
    h.update(&(i as u64).to_le_bytes());
    h.update(&src);
    let id = *h.finalize().as_bytes();
    let origin = {
        let mut o = [0u8; 32];
        o[..4].copy_from_slice(&i.to_le_bytes());
        o
    };
    Claim {
        id,
        fingerprint: fp,
        origin,
        kind: ClaimKind::Observation {
            sensor_type: 1,
            data: i.to_le_bytes().to_vec(),
        },
        confidence: LogOdds::new(2000),
        evidence_source: src,
        tick: i as u64,
    }
}

fn main() {
    let n = 1000u32;
    let iterations = 100;

    println!("=== L3 Hot-Path Profiling ===");
    println!("N = {} claims, {} iterations averaged\n", n, iterations);

    // Build graph and claims
    let mut graph = KnowledgeGraph::new();
    let mut claims = Vec::with_capacity(n as usize);
    let mut tracker = InMemoryReputationTracker::new();
    let anchor = [255u8; 32];
    tracker.update(&anchor, ReputationEvent::ClaimConfirmed);
    tracker.update(&anchor, ReputationEvent::ClaimConfirmed);
    tracker.update(&anchor, ReputationEvent::ClaimConfirmed);

    for i in 0..n {
        let claim = make_claim(i);
        tracker.delegate(&anchor, &claim.origin, Reputation::from_bps(5000));
        claims.push(claim);
        if i > 0 {
            graph.add_edge(EpistemicEdge {
                from: i - 1,
                to: i,
                relation: if i % 7 == 0 {
                    Relation::Contradicts
                } else {
                    Relation::Supports
                },
                strength: Reputation::from_bps(8000),
            });
        }
        // Add some cross-edges for realism
        if i > 5 && i % 3 == 0 {
            graph.add_edge(EpistemicEdge {
                from: i - 5,
                to: i,
                relation: Relation::DerivedFrom,
                strength: Reputation::from_bps(6000),
            });
        }
    }

    // Profile each step
    let mut t_base = std::time::Duration::ZERO;
    let mut t_cycle = std::time::Duration::ZERO;
    let mut t_pass1 = std::time::Duration::ZERO;
    let mut t_classify = std::time::Duration::ZERO;

    let mut last_state = BeliefState::default();

    for _ in 0..iterations {
        // Step 1: Base trust computation
        let start = Instant::now();
        let mut base_trust = rustc_hash::FxHashMap::default();
        for (i, claim) in claims.iter().enumerate() {
            let rep = tracker.reputation(&claim.origin);
            base_trust.insert(i as u32, rep.weight_evidence(claim.confidence));
        }
        t_base += start.elapsed();

        // Step 2: Cycle detection
        let start = Instant::now();
        let _cyclic = graph.cyclic_edge_indices();
        t_cycle += start.elapsed();

        // Step 3+4: Trust propagation (pass 1 + pass 2 combined)
        let start = Instant::now();
        let propagated = graph.propagate_trust_full(&base_trust, 5, 5000, &claims, &tracker);
        t_pass1 += start.elapsed(); // Includes both passes

        // Step 5: Classification
        let start = Instant::now();
        let engine = LogOddsBeliefEngine::default();
        let mut state = BeliefState::default();
        for (i, _claim) in claims.iter().enumerate() {
            let arena_id = i as u32;
            let final_logodds = propagated
                .get(&arena_id)
                .copied()
                .unwrap_or(LogOdds::NEUTRAL);
            if final_logodds.value() >= engine.accept_threshold.value() {
                state.accepted.push(arena_id);
            } else if final_logodds.value() <= engine.reject_threshold.value() {
                state.rejected.push(arena_id);
            } else {
                state.uncertain.push(arena_id);
            }
        }
        t_classify += start.elapsed();
        last_state = state;
    }

    let total = t_base + t_cycle + t_pass1 + t_classify;

    println!("| Step                        | Time (µs) | % of Total |");
    println!("|-----------------------------|-----------|------------|");
    print_row("Base trust (rep × conf)", t_base, total, iterations);
    print_row("Cycle detection (DFS)", t_cycle, total, iterations);
    print_row("Trust propagation (2-pass)", t_pass1, total, iterations);
    print_row("Classification", t_classify, total, iterations);
    println!("|-----------------------------|-----------|------------|");
    print_row("TOTAL", total, total, iterations);

    println!(
        "\nResults: accepted={}, rejected={}, uncertain={}",
        last_state.accepted.len(),
        last_state.rejected.len(),
        last_state.uncertain.len()
    );

    // Memory footprint
    let claim_size = std::mem::size_of::<Claim>();
    let edge_size = std::mem::size_of::<EpistemicEdge>();
    println!("\n| Metric              | Value   |");
    println!("|---------------------|---------|");
    println!("| Claim size (bytes)  | {:>7} |", claim_size);
    println!("| Edge size (bytes)   | {:>7} |", edge_size);
    println!("| Claims total        | {:>7} |", claims.len());
    println!("| Edges total         | {:>7} |", graph.edges().len());
    println!(
        "| Graph memory (est.) | {:>5} B |",
        claims.len() * claim_size + graph.edges().len() * edge_size
    );
}

fn print_row(label: &str, dur: std::time::Duration, total: std::time::Duration, iters: u32) {
    let avg_us = dur.as_micros() as f64 / iters as f64;
    let pct = if total.as_nanos() > 0 {
        dur.as_nanos() as f64 / total.as_nanos() as f64 * 100.0
    } else {
        0.0
    };
    println!("| {:<27} | {:>7.1} | {:>8.1}%  |", label, avg_us, pct);
}
