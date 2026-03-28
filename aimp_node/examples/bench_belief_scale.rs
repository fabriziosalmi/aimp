//! L3 Scalability Benchmarks
//!
//! Measures propagation latency across varying graph sizes and densities
//! to empirically validate O(V+E) complexity claims.
//!
//! Usage: cargo run --release --example bench_belief_scale

use aimp_node::epistemic::*;
use std::time::Instant;

fn make_claim(i: u32) -> Claim {
    let mut src = [0u8; 32];
    src[..4].copy_from_slice(&i.to_le_bytes());
    let hash = blake3::hash(&i.to_le_bytes());
    let mut primary = [0u8; 16];
    primary.copy_from_slice(&hash.as_bytes()[..16]);
    let fp = SemanticFingerprint {
        primary,
        secondary: i as u64,
    };
    let mut h = blake3::Hasher::new();
    h.update(&fp.primary);
    h.update(&(i as u64).to_le_bytes());
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

fn setup_tracker(claims: &[Claim]) -> InMemoryReputationTracker {
    let mut tracker = InMemoryReputationTracker::new();
    let anchor = [255u8; 32];
    tracker.update(&anchor, ReputationEvent::ClaimConfirmed);
    tracker.update(&anchor, ReputationEvent::ClaimConfirmed);
    tracker.update(&anchor, ReputationEvent::ClaimConfirmed);
    for claim in claims {
        tracker.delegate(&anchor, &claim.origin, Reputation::from_bps(5000));
    }
    tracker
}

fn base_trust(
    claims: &[Claim],
    tracker: &InMemoryReputationTracker,
) -> rustc_hash::FxHashMap<u32, LogOdds> {
    let mut bt = rustc_hash::FxHashMap::default();
    for (i, claim) in claims.iter().enumerate() {
        let rep = tracker.reputation(&claim.origin);
        bt.insert(i as u32, rep.weight_evidence(claim.confidence));
    }
    bt
}

fn bench_propagation(
    label: &str,
    graph: &KnowledgeGraph,
    claims: &[Claim],
    tracker: &InMemoryReputationTracker,
    iters: u32,
) {
    let bt = base_trust(claims, tracker);
    let mut total = std::time::Duration::ZERO;
    for _ in 0..iters {
        let start = Instant::now();
        let _ = graph.propagate_trust_full(&bt, 5, 5000, claims, tracker);
        total += start.elapsed();
    }
    let avg_us = total.as_micros() as f64 / iters as f64;
    let edges = graph.edges().len();
    println!(
        "| {:<40} | {:>8.1} µs | {:>6} V | {:>8} E |",
        label,
        avg_us,
        claims.len(),
        edges
    );
}

fn bench_cycle_detection(label: &str, graph: &KnowledgeGraph, claims_n: usize, iters: u32) {
    let mut total = std::time::Duration::ZERO;
    for _ in 0..iters {
        let start = Instant::now();
        let _ = graph.cyclic_edge_indices();
        total += start.elapsed();
    }
    let avg_us = total.as_micros() as f64 / iters as f64;
    println!(
        "| {:<40} | {:>8.1} µs | {:>6} V | {:>8} E |",
        label,
        avg_us,
        claims_n,
        graph.edges().len()
    );
}

fn bench_belief_engine(
    label: &str,
    graph: &KnowledgeGraph,
    claims: &[Claim],
    tracker: &InMemoryReputationTracker,
    iters: u32,
) {
    let engine = LogOddsBeliefEngine::default();
    let mut total = std::time::Duration::ZERO;
    for _ in 0..iters {
        let start = Instant::now();
        let _ = engine.compute(claims, graph, tracker);
        total += start.elapsed();
    }
    let avg_us = total.as_micros() as f64 / iters as f64;
    println!(
        "| {:<40} | {:>8.1} µs | {:>6} V | {:>8} E |",
        label,
        avg_us,
        claims.len(),
        graph.edges().len()
    );
}

fn main() {
    let iters = 50;

    println!("=== L3 Scalability Benchmarks ===");
    println!("Averaged over {} iterations\n", iters);

    // ── 1. Sparse graphs (chain topology) ──
    println!("--- Trust Propagation: Sparse (chain) ---");
    println!(
        "| {:<40} | {:>11} | {:>8} | {:>10} |",
        "Configuration", "Latency", "Vertices", "Edges"
    );
    println!("|{:-<42}|{:-<13}|{:-<10}|{:-<12}|", "", "", "", "");

    for &n in &[10u32, 100, 1000, 10000] {
        let claims: Vec<Claim> = (0..n).map(make_claim).collect();
        let mut graph = KnowledgeGraph::new();
        for i in 1..n {
            graph.add_edge(EpistemicEdge {
                from: i - 1,
                to: i,
                relation: Relation::Supports,
                strength: Reputation::from_bps(8000),
            });
        }
        let tracker = setup_tracker(&claims);
        bench_propagation(
            &format!("{} claims, sparse", n),
            &graph,
            &claims,
            &tracker,
            iters,
        );
    }

    // ── 2. Dense graphs ──
    println!("\n--- Trust Propagation: Dense ---");
    println!(
        "| {:<40} | {:>11} | {:>8} | {:>10} |",
        "Configuration", "Latency", "Vertices", "Edges"
    );
    println!("|{:-<42}|{:-<13}|{:-<10}|{:-<12}|", "", "", "", "");

    for &n in &[10u32, 50, 100, 200] {
        let claims: Vec<Claim> = (0..n).map(make_claim).collect();
        let mut graph = KnowledgeGraph::new();
        for i in 0..n {
            for j in 0..i {
                graph.add_edge(EpistemicEdge {
                    from: j,
                    to: i,
                    relation: Relation::Supports,
                    strength: Reputation::from_bps(7000),
                });
            }
        }
        let tracker = setup_tracker(&claims);
        bench_propagation(
            &format!("{} claims, dense ({}E)", n, n * (n - 1) / 2),
            &graph,
            &claims,
            &tracker,
            iters,
        );
    }

    // ── 3. Cycle detection overhead ──
    println!("\n--- Cycle Detection (increasing cycle density) ---");
    println!(
        "| {:<40} | {:>11} | {:>8} | {:>10} |",
        "Configuration", "Latency", "Vertices", "Edges"
    );
    println!("|{:-<42}|{:-<13}|{:-<10}|{:-<12}|", "", "", "", "");

    for &n in &[100u32, 500, 1000] {
        // Ring topology = 1 big cycle
        let mut graph = KnowledgeGraph::new();
        for i in 0..n {
            graph.add_edge(EpistemicEdge {
                from: i,
                to: (i + 1) % n,
                relation: Relation::Supports,
                strength: Reputation::from_bps(8000),
            });
        }
        bench_cycle_detection(
            &format!("{}-node ring (1 cycle)", n),
            &graph,
            n as usize,
            iters,
        );
    }

    // ── 4. Full pipeline (BeliefEngine) ──
    println!("\n--- Full Belief Engine Pipeline ---");
    println!(
        "| {:<40} | {:>11} | {:>8} | {:>10} |",
        "Configuration", "Latency", "Vertices", "Edges"
    );
    println!("|{:-<42}|{:-<13}|{:-<10}|{:-<12}|", "", "", "", "");

    for &n in &[10u32, 100, 1000, 5000] {
        let claims: Vec<Claim> = (0..n).map(make_claim).collect();
        let mut graph = KnowledgeGraph::new();
        for i in 1..n {
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
        let tracker = setup_tracker(&claims);
        bench_belief_engine(
            &format!("{} claims, mixed graph", n),
            &graph,
            &claims,
            &tracker,
            iters,
        );
    }

    // ── 5. Memory footprint ──
    println!("\n--- Memory Footprint ---");
    println!("| Metric                | Value       |");
    println!("|-----------------------|-------------|");
    println!(
        "| sizeof(Claim)         | {:>7} B   |",
        std::mem::size_of::<Claim>()
    );
    println!(
        "| sizeof(EpistemicEdge)  | {:>7} B   |",
        std::mem::size_of::<EpistemicEdge>()
    );
    println!(
        "| sizeof(LogOdds)        | {:>7} B   |",
        std::mem::size_of::<LogOdds>()
    );
    println!(
        "| sizeof(Reputation)     | {:>7} B   |",
        std::mem::size_of::<Reputation>()
    );
    println!(
        "| sizeof(BeliefState)    | {:>7} B   |",
        std::mem::size_of::<BeliefState>()
    );
    println!(
        "| sizeof(KnowledgeGraph) | {:>7} B   |",
        std::mem::size_of::<KnowledgeGraph>()
    );
}
