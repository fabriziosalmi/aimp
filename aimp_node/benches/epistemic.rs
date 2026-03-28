//! L3 Epistemic Layer — Criterion Benchmarks
//!
//! Mirrors `benches/core.rs` for L2. Every number in the paper comes from here.

use aimp_node::epistemic::*;
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};

// ─── Helpers ────────────────────────────────────────────────

fn make_fingerprint(data: &[u8], sensor: u8) -> SemanticFingerprint {
    let hash = blake3::hash(data);
    let mut primary = [0u8; 16];
    primary.copy_from_slice(&hash.as_bytes()[..16]);
    let mut feat = blake3::Hasher::new();
    feat.update(&[sensor]);
    let secondary = u64::from_le_bytes(feat.finalize().as_bytes()[..8].try_into().unwrap());
    SemanticFingerprint { primary, secondary }
}

fn make_claim(sensor: u8, data: &[u8], logodds: i32, tick: u64, source: [u8; 32]) -> Claim {
    let fp = make_fingerprint(data, sensor);
    let mut h = blake3::Hasher::new();
    h.update(&fp.primary);
    h.update(&tick.to_le_bytes());
    h.update(&source);
    let id = *h.finalize().as_bytes();
    Claim {
        id,
        fingerprint: fp,
        origin: [1u8; 32],
        kind: ClaimKind::Observation {
            sensor_type: sensor,
            data: data.to_vec(),
        },
        confidence: LogOdds::new(logodds),
        evidence_source: source,
        tick,
    }
}

fn build_sparse_graph(n: u32) -> (KnowledgeGraph, Vec<Claim>) {
    let mut graph = KnowledgeGraph::new();
    let mut claims = Vec::with_capacity(n as usize);
    for i in 0..n {
        let mut src = [0u8; 32];
        src[..4].copy_from_slice(&i.to_le_bytes());
        claims.push(make_claim(1, b"data", 2000, i as u64, src));
        if i > 0 {
            graph.add_edge(EpistemicEdge {
                from: i - 1,
                to: i,
                relation: Relation::Supports,
                strength: Reputation::from_bps(8000),
            });
        }
    }
    (graph, claims)
}

fn build_dense_graph(n: u32) -> (KnowledgeGraph, Vec<Claim>) {
    let mut graph = KnowledgeGraph::new();
    let mut claims = Vec::with_capacity(n as usize);
    for i in 0..n {
        let mut src = [0u8; 32];
        src[..4].copy_from_slice(&i.to_le_bytes());
        claims.push(make_claim(1, b"data", 2000, i as u64, src));
        // Connect every node to every previous node (dense)
        for j in 0..i {
            graph.add_edge(EpistemicEdge {
                from: j,
                to: i,
                relation: Relation::Supports,
                strength: Reputation::from_bps(7000),
            });
        }
    }
    (graph, claims)
}

// ─── 1. LogOdds Aggregation ────────────────────────────────

fn bench_logodds_aggregate(c: &mut Criterion) {
    let mut group = c.benchmark_group("logodds_aggregate");
    for n in [10, 100, 1000] {
        let evidence: Vec<LogOdds> = (0..n).map(|i| LogOdds::new((i % 5000) - 2500)).collect();
        group.bench_with_input(BenchmarkId::from_parameter(n), &evidence, |b, ev| {
            b.iter(|| LogOdds::aggregate(black_box(ev)));
        });
    }
    group.finish();
}

// ─── 2. LogOdds from_percent ───────────────────────────────

fn bench_logodds_from_percent(c: &mut Criterion) {
    c.bench_function("logodds_from_percent", |b| {
        b.iter(|| {
            for pct in 0..=100u8 {
                black_box(LogOdds::from_percent(pct));
            }
        });
    });
}

// ─── 3. Fingerprint Compute ────────────────────────────────

fn bench_fingerprint_compute(c: &mut Criterion) {
    let data = vec![0u8; 256];
    c.bench_function("fingerprint_compute", |b| {
        b.iter(|| make_fingerprint(black_box(&data), 1));
    });
}

// ─── 4. Graph Add Edge ─────────────────────────────────────

fn bench_graph_add_edge(c: &mut Criterion) {
    let mut group = c.benchmark_group("graph_add_edge");
    for n in [10, 100, 1000] {
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter(|| {
                let mut graph = KnowledgeGraph::new();
                for i in 0..n {
                    graph.add_edge(EpistemicEdge {
                        from: i,
                        to: i + 1,
                        relation: Relation::Supports,
                        strength: Reputation::from_bps(8000),
                    });
                }
                black_box(&graph);
            });
        });
    }
    group.finish();
}

// ─── 5. Cycle Detection ────────────────────────────────────

fn bench_graph_detect_cycles(c: &mut Criterion) {
    let mut group = c.benchmark_group("graph_detect_cycles");
    for n in [10u32, 100, 1000] {
        let (graph, _) = build_sparse_graph(n);
        group.bench_with_input(BenchmarkId::from_parameter(n), &graph, |b, g| {
            b.iter(|| g.detect_cycles());
        });
    }
    group.finish();
}

// ─── 6. Cyclic Edge Identification ─────────────────────────

fn bench_graph_cyclic_edges(c: &mut Criterion) {
    let mut group = c.benchmark_group("graph_cyclic_edges");
    for n in [10u32, 100, 500] {
        // Create graph with some cycles
        let mut graph = KnowledgeGraph::new();
        for i in 0..n {
            graph.add_edge(EpistemicEdge {
                from: i,
                to: (i + 1) % n,
                relation: Relation::Supports,
                strength: Reputation::from_bps(8000),
            });
        }
        group.bench_with_input(BenchmarkId::from_parameter(n), &graph, |b, g| {
            b.iter(|| g.cyclic_edge_indices());
        });
    }
    group.finish();
}

// ─── 7. Trust Propagation (the key number) ─────────────────

fn bench_trust_propagation(c: &mut Criterion) {
    let mut group = c.benchmark_group("trust_propagation");

    for &(n, label) in &[
        (10u32, "10_sparse"),
        (100, "100_sparse"),
        (1000, "1000_sparse"),
    ] {
        let (graph, claims) = build_sparse_graph(n);
        let mut tracker = InMemoryReputationTracker::new();
        // Give all nodes reputation via delegation from anchor
        let anchor = [255u8; 32];
        tracker.update(&anchor, ReputationEvent::ClaimConfirmed);
        tracker.update(&anchor, ReputationEvent::ClaimConfirmed);
        tracker.update(&anchor, ReputationEvent::ClaimConfirmed);
        for claim in &claims {
            tracker.delegate(&anchor, &claim.origin, Reputation::from_bps(5000));
        }
        let mut base = rustc_hash::FxHashMap::default();
        for (i, claim) in claims.iter().enumerate() {
            let rep = tracker.reputation(&claim.origin);
            base.insert(i as u32, rep.weight_evidence(claim.confidence));
        }

        group.bench_with_input(
            BenchmarkId::new("sparse", label),
            &(&graph, &claims, &tracker, &base),
            |b, (g, c, t, bt)| {
                b.iter(|| g.propagate_trust_full(black_box(bt), 5, 5000, black_box(c), *t));
            },
        );
    }

    for &(n, label) in &[(10u32, "10_dense"), (50, "50_dense"), (100, "100_dense")] {
        let (graph, claims) = build_dense_graph(n);
        let mut tracker = InMemoryReputationTracker::new();
        let anchor = [255u8; 32];
        tracker.update(&anchor, ReputationEvent::ClaimConfirmed);
        tracker.update(&anchor, ReputationEvent::ClaimConfirmed);
        tracker.update(&anchor, ReputationEvent::ClaimConfirmed);
        for claim in &claims {
            tracker.delegate(&anchor, &claim.origin, Reputation::from_bps(5000));
        }
        let mut base = rustc_hash::FxHashMap::default();
        for (i, claim) in claims.iter().enumerate() {
            let rep = tracker.reputation(&claim.origin);
            base.insert(i as u32, rep.weight_evidence(claim.confidence));
        }

        group.bench_with_input(
            BenchmarkId::new("dense", label),
            &(&graph, &claims, &tracker, &base),
            |b, (g, c, t, bt)| {
                b.iter(|| g.propagate_trust_full(black_box(bt), 5, 5000, black_box(c), *t));
            },
        );
    }

    group.finish();
}

// ─── 8. ExactMatchReducer ──────────────────────────────────

fn bench_reducer_exact_match(c: &mut Criterion) {
    let mut group = c.benchmark_group("reducer_exact_match");
    let reducer = ExactMatchReducer;

    for n in [10, 100, 1000] {
        let claims: Vec<Claim> = (0..n)
            .map(|i| {
                let mut src = [0u8; 32];
                src[0] = (i % 256) as u8;
                src[1] = (i / 256) as u8;
                make_claim(1, b"temp=20C", 2000, i as u64, src)
            })
            .collect();

        group.bench_with_input(BenchmarkId::from_parameter(n), &claims, |b, cls| {
            b.iter(|| reducer.reduce(black_box(cls)));
        });
    }
    group.finish();
}

// ─── 9. Full Belief Engine Pipeline ────────────────────────

fn bench_belief_engine(c: &mut Criterion) {
    let mut group = c.benchmark_group("belief_engine");
    let engine = LogOddsBeliefEngine::default();

    for &(n, label) in &[(10u32, "10"), (100, "100"), (1000, "1000")] {
        let (graph, claims) = build_sparse_graph(n);
        let mut tracker = InMemoryReputationTracker::new();
        let anchor = [255u8; 32];
        tracker.update(&anchor, ReputationEvent::ClaimConfirmed);
        tracker.update(&anchor, ReputationEvent::ClaimConfirmed);
        tracker.update(&anchor, ReputationEvent::ClaimConfirmed);
        for claim in &claims {
            tracker.delegate(&anchor, &claim.origin, Reputation::from_bps(5000));
        }

        group.bench_with_input(
            BenchmarkId::from_parameter(label),
            &(&graph, &claims, &tracker),
            |b, (g, c, t)| {
                b.iter(|| engine.compute(black_box(c), black_box(g), *t));
            },
        );
    }
    group.finish();
}

criterion_group!(
    epistemic_benches,
    bench_logodds_aggregate,
    bench_logodds_from_percent,
    bench_fingerprint_compute,
    bench_graph_add_edge,
    bench_graph_detect_cycles,
    bench_graph_cyclic_edges,
    bench_trust_propagation,
    bench_reducer_exact_match,
    bench_belief_engine,
);
criterion_main!(epistemic_benches);
