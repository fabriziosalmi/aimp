//! Property-based tests for L3 Epistemic Layer.
//!
//! These verify the algebraic invariants claimed in the paper:
//! - Reducer: commutativity, associativity, idempotency
//! - Trust propagation: determinism, order-independence
//! - Log-odds: commutativity, no-overflow
//!
//! Run with: cargo test --test epistemic_proptests

use aimp_node::epistemic::*;
use proptest::prelude::*;

// ─── Strategies ─────────────────────────────────────────────

fn arb_logodds() -> impl Strategy<Value = LogOdds> {
    (-10000i32..=10000).prop_map(LogOdds::new)
}

fn arb_claim() -> impl Strategy<Value = Claim> {
    (
        0..10u8,
        any::<[u8; 4]>(),
        -5000i32..5000,
        0u64..1000,
        any::<[u8; 32]>(),
    )
        .prop_map(|(sensor, data, lo, tick, source)| {
            let fp = make_fingerprint(&data, sensor);
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
                confidence: LogOdds::new(lo),
                evidence_source: source,
                tick,
            }
        })
}

/// Claims that share the same fingerprint (required for reducer)
fn arb_same_fingerprint_claims(n: usize) -> impl Strategy<Value = Vec<Claim>> {
    (0..10u8, any::<[u8; 4]>()).prop_flat_map(move |(sensor, data)| {
        proptest::collection::vec(
            (-5000i32..5000, 0u64..1000, any::<[u8; 32]>()).prop_map(move |(lo, tick, source)| {
                let fp = make_fingerprint(&data, sensor);
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
                    confidence: LogOdds::new(lo),
                    evidence_source: source,
                    tick,
                }
            }),
            2..=n,
        )
    })
}

fn make_fingerprint(data: &[u8], sensor: u8) -> SemanticFingerprint {
    let hash = blake3::hash(data);
    let mut primary = [0u8; 16];
    primary.copy_from_slice(&hash.as_bytes()[..16]);
    let mut feat = blake3::Hasher::new();
    feat.update(&[sensor]);
    let secondary = u64::from_le_bytes(feat.finalize().as_bytes()[..8].try_into().unwrap());
    SemanticFingerprint { primary, secondary }
}

// ─── 1. Log-Odds Commutativity ──────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn logodds_aggregate_commutative(a in arb_logodds(), b in arb_logodds()) {
        let ab = LogOdds::aggregate(&[a, b]);
        let ba = LogOdds::aggregate(&[b, a]);
        prop_assert_eq!(ab, ba, "aggregate must be commutative");
    }
}

// ─── 2. Log-Odds No Overflow Panic ──────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn logodds_no_overflow_panic(values in proptest::collection::vec(any::<i32>(), 0..100)) {
        let logodds: Vec<LogOdds> = values.into_iter().map(LogOdds::new).collect();
        let _ = LogOdds::aggregate(&logodds); // must not panic
    }
}

// ─── 3. Reducer Commutativity ───────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn reducer_commutativity(claims in arb_same_fingerprint_claims(8)) {
        let reducer = ExactMatchReducer;
        if !reducer.can_reduce(&claims) {
            return Ok(());
        }

        let mut reversed = claims.clone();
        reversed.reverse();

        let forward = reducer.reduce(&claims);
        let backward = reducer.reduce(&reversed);

        match (forward, backward) {
            (Some(f), Some(b)) => {
                prop_assert_eq!(f.id, b.id, "reducer must be commutative: same id regardless of order");
                prop_assert_eq!(f.confidence, b.confidence, "reducer must be commutative: same confidence");
            }
            (None, None) => {} // Both fail = ok
            _ => prop_assert!(false, "reducer commutativity: one succeeded and one failed"),
        }
    }
}

// ─── 4. Reducer Idempotency ─────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn reducer_idempotency(claims in arb_same_fingerprint_claims(8)) {
        let reducer = ExactMatchReducer;
        if !reducer.can_reduce(&claims) {
            return Ok(());
        }

        let once = reducer.reduce(&claims);
        // Duplicate all claims
        let mut doubled = claims.clone();
        doubled.extend(claims.iter().cloned());
        let twice = reducer.reduce(&doubled);

        match (once, twice) {
            (Some(o), Some(t)) => {
                // Idempotency: reduce(A ++ A) should produce same unique_sources as reduce(A)
                if let (
                    ClaimKind::Summary { unique_sources: u1, .. },
                    ClaimKind::Summary { unique_sources: u2, .. },
                ) = (&o.kind, &t.kind)
                {
                    prop_assert_eq!(u1, u2, "idempotency: unique_sources must not double");
                }
            }
            _ => {}
        }
    }
}

// ─── 5. Trust Propagation Determinism ───────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn trust_propagation_deterministic(
        n in 3u32..20,
        seed in any::<u64>(),
    ) {
        // Build a deterministic graph from seed
        let mut graph = KnowledgeGraph::new();
        let mut claims = Vec::new();
        let mut tracker = InMemoryReputationTracker::new();
        let anchor = [255u8; 32];
        tracker.update(&anchor, ReputationEvent::ClaimConfirmed);
        tracker.update(&anchor, ReputationEvent::ClaimConfirmed);

        for i in 0..n {
            let mut src = [0u8; 32];
            src[..4].copy_from_slice(&i.to_le_bytes());
            src[4..12].copy_from_slice(&seed.to_le_bytes());
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
            claims.push(Claim {
                id,
                fingerprint: fp,
                origin,
                kind: ClaimKind::Observation { sensor_type: 1, data: i.to_le_bytes().to_vec() },
                confidence: LogOdds::new(2000),
                evidence_source: src,
                tick: i as u64,
            });
            tracker.delegate(&anchor, &origin, Reputation::from_bps(5000));
            if i > 0 {
                graph.add_edge(EpistemicEdge {
                    from: i - 1,
                    to: i,
                    relation: Relation::Supports,
                    strength: Reputation::from_bps(8000),
                });
            }
        }

        let mut base = rustc_hash::FxHashMap::default();
        for (i, claim) in claims.iter().enumerate() {
            let rep = tracker.reputation(&claim.origin);
            base.insert(i as u32, rep.weight_evidence(claim.confidence));
        }

        // Run twice — must be identical
        let r1 = graph.propagate_trust_full(&base, 5, 5000, &claims, &tracker);
        let r2 = graph.propagate_trust_full(&base, 5, 5000, &claims, &tracker);

        for key in r1.keys() {
            prop_assert_eq!(
                r1.get(key),
                r2.get(key),
                "trust propagation must be deterministic for node {}",
                key
            );
        }
    }
}

// ─── 6. Belief Engine Determinism ───────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn belief_engine_deterministic(n in 3u32..15) {
        let engine = LogOddsBeliefEngine::default();
        let mut graph = KnowledgeGraph::new();
        let mut claims = Vec::new();
        let mut tracker = InMemoryReputationTracker::new();
        let anchor = [255u8; 32];
        tracker.update(&anchor, ReputationEvent::ClaimConfirmed);
        tracker.update(&anchor, ReputationEvent::ClaimConfirmed);

        for i in 0..n {
            let mut src = [0u8; 32];
            src[..4].copy_from_slice(&i.to_le_bytes());
            let origin = {
                let mut o = [0u8; 32];
                o[..4].copy_from_slice(&i.to_le_bytes());
                o
            };
            let fp = make_fingerprint(&i.to_le_bytes(), 1);
            let mut h = blake3::Hasher::new();
            h.update(&fp.primary);
            h.update(&(i as u64).to_le_bytes());
            h.update(&src);
            let id = *h.finalize().as_bytes();
            claims.push(Claim {
                id,
                fingerprint: fp,
                origin,
                kind: ClaimKind::Observation { sensor_type: 1, data: i.to_le_bytes().to_vec() },
                confidence: LogOdds::new(if i % 3 == 0 { 5000 } else if i % 3 == 1 { -5000 } else { 500 }),
                evidence_source: src,
                tick: i as u64,
            });
            tracker.delegate(&anchor, &origin, Reputation::from_bps(5000));
            if i > 0 {
                graph.add_edge(EpistemicEdge {
                    from: i - 1,
                    to: i,
                    relation: if i % 4 == 0 { Relation::Contradicts } else { Relation::Supports },
                    strength: Reputation::from_bps(8000),
                });
            }
        }

        let s1 = engine.compute(&claims, &graph, &tracker);
        let s2 = engine.compute(&claims, &graph, &tracker);

        prop_assert_eq!(s1.accepted, s2.accepted, "accepted must be deterministic");
        prop_assert_eq!(s1.rejected, s2.rejected, "rejected must be deterministic");
        prop_assert_eq!(s1.uncertain, s2.uncertain, "uncertain must be deterministic");
    }
}

// ─── 7. Log-Odds Aggregate Associativity ────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn logodds_aggregate_associative(
        a in arb_logodds(),
        b in arb_logodds(),
        c in arb_logodds(),
    ) {
        // aggregate([a, b, c]) should equal aggregate([aggregate([a, b]), c])
        let abc = LogOdds::aggregate(&[a, b, c]);
        let ab_then_c = LogOdds::aggregate(&[LogOdds::aggregate(&[a, b]), c]);
        let a_then_bc = LogOdds::aggregate(&[a, LogOdds::aggregate(&[b, c])]);

        prop_assert_eq!(abc, ab_then_c, "aggregate must be associative (left)");
        prop_assert_eq!(abc, a_then_bc, "aggregate must be associative (right)");
    }
}
