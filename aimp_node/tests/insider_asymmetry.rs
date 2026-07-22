//! Correlation Discounting: Coverage and Asymmetry Measurements
//!
//! Empirical investigation for github.com/fabriziosalmi/aimp/issues/8.
//!
//! v0.3.0 introduced Grid-Cell Correlation Discounting to prevent pathological
//! hyper-confidence when physically or semantically correlated sources report
//! concordant observations. This file measures:
//!
//!   A. that the discount works where it is applied (baseline, expected pass);
//!   B. which code paths it actually reaches;
//!   C. what an adversary gains from controlling its own self-declared
//!      `confidence` and `correlation_cell` metadata.
//!
//! Every measurement is a direct call into the production API. No mocks.
//!
//! Run with: cargo test --test insider_asymmetry -- --nocapture --test-threads=1

use aimp_node::epistemic::*;

const DISCOUNT: u16 = 3000; // protocol default: 30%
const HONEST_CONF: i32 = 847; // == LogOdds::from_percent(70)
const HONEST_REP: u16 = 6000; // solid, unremarkable operator
const INSIDER_REP: u16 = 9500; // months of earned, legitimate reputation

fn fingerprint(data: &[u8], sensor: u8) -> SemanticFingerprint {
    let hash = blake3::hash(data);
    let mut primary = [0u8; 16];
    primary.copy_from_slice(&hash.as_bytes()[..16]);
    let mut feat = blake3::Hasher::new();
    feat.update(&[sensor]);
    let secondary = u64::from_le_bytes(feat.finalize().as_bytes()[..8].try_into().unwrap());
    SemanticFingerprint { primary, secondary }
}

fn claim(seq: u64, conf: i32, cell: Option<u64>, origin: [u8; 32], tick: u64) -> Claim {
    let mut id = [0u8; 32];
    id[..8].copy_from_slice(&seq.to_le_bytes());
    let mut src = [0u8; 32];
    src[..8].copy_from_slice(&(seq ^ 0xABCD).to_le_bytes());
    Claim {
        id,
        fingerprint: fingerprint(b"temperature", 1),
        origin,
        kind: ClaimKind::Observation {
            sensor_type: 1,
            data: b"22C".to_vec(),
        },
        confidence: LogOdds::new(conf),
        evidence_source: src,
        tick,
        correlation_cell: cell.map(CorrelationCell),
        embedding: None,
        embedding_version: 0,
    }
}

/// Claim with distinct CONTENT, so its fingerprint differs from its siblings.
///
/// Genuinely independent sources assert different things; sources asserting
/// byte-identical content must, by the protocol's determinism rule, produce
/// byte-identical embeddings. Fixtures that mix identical content with differing
/// embeddings model an attack, not an honest deployment.
fn claim_with_content(
    seq: u64,
    conf: i32,
    cell: Option<u64>,
    origin: [u8; 32],
    tick: u64,
    content: &[u8],
) -> Claim {
    let mut c = claim(seq, conf, cell, origin, tick);
    c.fingerprint = fingerprint(content, 1);
    c.kind = ClaimKind::Observation {
        sensor_type: 1,
        data: content.to_vec(),
    };
    c
}

fn origin_of(i: u64) -> [u8; 32] {
    let mut o = [7u8; 32];
    o[..8].copy_from_slice(&i.to_le_bytes());
    o
}

/// Aggregate N correlated sources through the production discounting function.
fn honest_cluster(n: usize, conf: i32, rep_bps: u16, discount_bps: u16) -> i64 {
    let rep = Reputation::from_bps(rep_bps);
    let cell = Some(CorrelationCell(42));
    let evidence: Vec<(LogOdds, Option<CorrelationCell>, ClaimHash)> = (0..n as u64)
        .map(|i| {
            let mut id = [0u8; 32];
            id[..8].copy_from_slice(&i.to_le_bytes());
            (rep.weight_evidence(LogOdds::new(conf)), cell, id)
        })
        .collect();
    LogOdds::aggregate_correlated(&evidence, discount_bps).value() as i64
}

// ════════════════════════════════════════════════════════════════════
// SECTION A — Baseline: the discount works where it is applied.
// ════════════════════════════════════════════════════════════════════

/// A1. The honest cluster saturates at 1/(1-d) times a single source.
/// This is the intended v0.3.0 behaviour and the baseline for everything below.
#[test]
fn a1_honest_cluster_saturates() {
    println!("\n=== A1. Honest cluster ceiling (conf=70%, rep=6000bps, d=30%) ===");
    println!("{:>8}  {:>12}  {:>10}", "N", "aggregate", "x single");
    let single = honest_cluster(1, HONEST_CONF, HONEST_REP, DISCOUNT);
    let mut last = 0;
    for n in [1usize, 2, 3, 5, 10, 100, 1000, 10_000] {
        let agg = honest_cluster(n, HONEST_CONF, HONEST_REP, DISCOUNT);
        println!("{:>8}  {:>12}  {:>9.3}x", n, agg, agg as f64 / single as f64);
        last = agg;
    }
    assert!(
        last <= single * 15 / 10,
        "cluster must saturate below 1.5x a single source"
    );
}

// ════════════════════════════════════════════════════════════════════
// SECTION B — Coverage: which paths does the discount actually reach?
// ════════════════════════════════════════════════════════════════════

/// B1. THE CENTRAL MEASUREMENT.
///
/// The same N correlated sources are evaluated on the two production paths:
///
///   - the REDUCER path (`aggregate_correlated`), which applies the discount;
///   - the PROPAGATION path (`propagate_trust_full`), where each source emits
///     a Supports edge toward a shared conclusion.
///
/// Pass 1 normalizes OUTGOING edge strength per source (Markovian flow), but
/// nothing normalizes INCOMING edges at a target and nothing consults
/// `correlation_cell`. If the discount does not reach this path, correlated
/// support should amplify roughly linearly in N — the exact pathology v0.3.0
/// was introduced to eliminate.
#[test]
fn b1_supports_propagation_ignores_correlation() {
    println!("\n=== B1. Same correlated sources, two paths ===");
    println!(
        "{:>8}  {:>16}  {:>18}  {:>12}",
        "N", "reducer (capped)", "propagation trust", "ratio"
    );

    let mut ratios = Vec::new();
    for n in [1usize, 2, 5, 10, 50, 100] {
        // ---- reducer path ----
        let reduced = honest_cluster(n, HONEST_CONF, HONEST_REP, DISCOUNT);

        // ---- propagation path: N correlated sources all Supporting claim 0 ----
        let mut tracker = InMemoryReputationTracker::new();
        let mut claims = vec![claim(0, HONEST_CONF, Some(42), origin_of(0), 1)];
        tracker.set_reputation(&origin_of(0), Reputation::from_bps(HONEST_REP));
        for i in 1..=n as u64 {
            claims.push(claim(i, HONEST_CONF, Some(42), origin_of(i), 1));
            tracker.set_reputation(&origin_of(i), Reputation::from_bps(HONEST_REP));
        }

        let edges: Vec<RawEpistemicEdge> = (1..=n as u64)
            .map(|i| RawEpistemicEdge {
                from_hash: claims[i as usize].id,
                from_fingerprint: claims[i as usize].fingerprint,
                to_hash: claims[0].id,
                to_fingerprint: claims[0].fingerprint,
                relation: Relation::Supports,
                strength: Reputation::FULL,
            })
            .collect();

        let graph = KnowledgeGraph::build_from_claims(&claims, &edges);
        let base: rustc_hash::FxHashMap<ClaimArenaId, LogOdds> = claims
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let rep = tracker.reputation(&c.origin);
                (i as ClaimArenaId, rep.weight_evidence(c.confidence))
            })
            .collect();

        let propagated = graph.propagate_trust_full(&base, 5, 5000, &claims, &tracker);
        let target = propagated.get(&0).copied().unwrap_or(LogOdds::NEUTRAL).value() as i64;

        let ratio = target as f64 / reduced as f64;
        println!("{:>8}  {:>16}  {:>18}  {:>11.2}x", n, reduced, target, ratio);
        ratios.push((n, target));
    }

    // Both paths must now be bounded in N. The reducer ceiling is flat; after the
    // fix the propagation ceiling must be flat too.
    let (_, t_small) = ratios[3]; // N=10
    let (_, t_large) = ratios[ratios.len() - 1]; // N=100
    assert!(
        t_large < t_small * 105 / 100,
        "propagation must saturate in N once correlation is discounted: {} -> {}",
        t_small,
        t_large
    );
}

/// B1b. DECISIVE CONTROL for B1.
///
/// Objection to B1: "trust growing linearly in N is CORRECT Bayesian behaviour
/// for independent sources — you have shown growth, not that the cell is
/// ignored."
///
/// This control answers it. The topology, reputations and confidences are held
/// identical; only `correlation_cell` changes:
///
///   - variant SAME: all N supporters declare cell 42 (fully correlated);
///   - variant DIFF: supporter i declares cell i (fully independent);
///   - variant NONE: all supporters declare no cell.
///
/// If the propagation path consulted correlation metadata, SAME would be
/// discounted far below DIFF. Byte-identical results prove the field is never
/// read on this path.
#[test]
fn b1b_propagation_identical_regardless_of_cell() {
    println!("\n=== B1b. Control: same topology, different correlation metadata ===");

    fn propagated_trust(n: u64, cell_for: impl Fn(u64) -> Option<u64>) -> i64 {
        let mut tracker = InMemoryReputationTracker::new();
        let mut claims = vec![claim(0, HONEST_CONF, Some(42), origin_of(0), 1)];
        tracker.set_reputation(&origin_of(0), Reputation::from_bps(HONEST_REP));
        for i in 1..=n {
            claims.push(claim(i, HONEST_CONF, cell_for(i), origin_of(i), 1));
            tracker.set_reputation(&origin_of(i), Reputation::from_bps(HONEST_REP));
        }
        let edges: Vec<RawEpistemicEdge> = (1..=n)
            .map(|i| RawEpistemicEdge {
                from_hash: claims[i as usize].id,
                from_fingerprint: claims[i as usize].fingerprint,
                to_hash: claims[0].id,
                to_fingerprint: claims[0].fingerprint,
                relation: Relation::Supports,
                strength: Reputation::FULL,
            })
            .collect();
        let graph = KnowledgeGraph::build_from_claims(&claims, &edges);
        let base: rustc_hash::FxHashMap<ClaimArenaId, LogOdds> = claims
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let rep = tracker.reputation(&c.origin);
                (i as ClaimArenaId, rep.weight_evidence(c.confidence))
            })
            .collect();
        graph
            .propagate_trust_full(&base, 5, 5000, &claims, &tracker)
            .get(&0)
            .copied()
            .unwrap_or(LogOdds::NEUTRAL)
            .value() as i64
    }

    println!("{:>8}  {:>14}  {:>14}  {:>14}", "N", "SAME cell", "DIFF cells", "NO cell");
    let mut same_series = Vec::new();
    for n in [2u64, 10, 100] {
        let same = propagated_trust(n, |_| Some(42));
        let diff = propagated_trust(n, Some);
        let none = propagated_trust(n, |_| None);
        println!("{:>8}  {:>14}  {:>14}  {:>14}", n, same, diff, none);
        // Correlated support must always be strictly below independent support.
        // The gap cannot be large at N=2 (only one contribution is discounted),
        // so the strong bound is asserted at N=100 after the loop.
        assert!(
            same < diff,
            "N={}: correlated support ({}) must be below independent ({})",
            n,
            same,
            diff
        );
        // Independent sources unaffected: distinct cells and absent cells are
        // both singleton groups and must stay identical to each other.
        assert_eq!(
            diff, none,
            "N={}: independent sources must keep full weight",
            n
        );
        same_series.push(same);
    }

    // ── Third tier: independence that is SUBSTANTIATED (distinct embedding
    // clusters) must still accumulate linearly. Level 2 must bound unverified
    // independence without punishing verified independence.
    let assessed_100 = {
        use aimp_node::semantic_topology::QuantizedEmbedding;
        let mut tracker = InMemoryReputationTracker::new();
        let mut claims = vec![claim(0, HONEST_CONF, Some(42), origin_of(0), 1)];
        tracker.set_reputation(&origin_of(0), Reputation::from_bps(HONEST_REP));
        for i in 1..=100u64 {
            // Distinct content AND mutually distant embeddings: genuinely
            // independent observations, consistently reported.
            let mut c = claim_with_content(
                i,
                HONEST_CONF,
                None,
                origin_of(i),
                1,
                format!("reading-{}", i).as_bytes(),
            );
            c.embedding = Some(QuantizedEmbedding::new([
                i.wrapping_mul(0x9E37_79B9_7F4A_7C15),
                i.wrapping_mul(0xBF58_476D_1CE4_E5B9),
                i.wrapping_mul(0x94D0_49BB_1331_11EB),
                i.wrapping_mul(0x2545_F491_4F6C_DD1D),
            ]));
            claims.push(c);
            tracker.set_reputation(&origin_of(i), Reputation::from_bps(HONEST_REP));
        }
        let edges: Vec<RawEpistemicEdge> = (1..=100u64)
            .map(|i| RawEpistemicEdge {
                from_hash: claims[i as usize].id,
                from_fingerprint: claims[i as usize].fingerprint,
                to_hash: claims[0].id,
                to_fingerprint: claims[0].fingerprint,
                relation: Relation::Supports,
                strength: Reputation::FULL,
            })
            .collect();
        let graph = KnowledgeGraph::build_from_claims(&claims, &edges);
        let base: rustc_hash::FxHashMap<ClaimArenaId, LogOdds> = claims
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let rep = tracker.reputation(&c.origin);
                (i as ClaimArenaId, rep.weight_evidence(c.confidence))
            })
            .collect();
        graph
            .propagate_trust_full(&base, 5, 5000, &claims, &tracker)
            .get(&0)
            .copied()
            .unwrap_or(LogOdds::NEUTRAL)
            .value() as i64
    };

    let same_100 = same_series[2];
    let diff_100 = propagated_trust(100, Some);
    println!("\nN=100 three tiers:");
    println!("  correlated (same cluster)      : {}", same_100);
    println!("  unverified independence        : {}", diff_100);
    println!("  substantiated independence     : {}", assessed_100);

    // Tier ordering is the whole point of the design.
    assert!(
        same_100 < diff_100,
        "correlated ({}) must rank below unverified independence ({})",
        same_100,
        diff_100
    );
    assert!(
        diff_100 * 5 < assessed_100,
        "unverified independence ({}) must rank far below substantiated ({})",
        diff_100,
        assessed_100
    );

    // Correlated support must SATURATE in N, as the reducer path already does.
    let growth = same_series[2] as f64 / same_series[1] as f64; // N=100 vs N=10
    println!(
        "\ncorrelated growth N=10 -> N=100: {:.4}x (must be ~1.0)",
        growth
    );
    assert!(
        growth < 1.05,
        "correlated support must saturate in N, grew {:.3}x",
        growth
    );
}

/// B1c. DETERMINISM of the correlation-discounted propagation.
///
/// The fix groups incoming contributions in a hash map keyed by target. Map
/// iteration order is unspecified, so determinism must not depend on it: each
/// target is totalled independently and inserted once, and ranking within a
/// cell uses a total order (|contribution| desc, ClaimHash asc).
///
/// This test permutes the EDGE INSERTION ORDER — which changes both map layout
/// and per-target gather order — and requires bit-identical output. Divergence
/// here would split the mesh, so this is consensus-critical.
#[test]
fn b1c_propagation_is_order_independent() {
    println!("\n=== B1c. Determinism under edge-order permutation ===");
    let n = 40u64;

    let run = |perm: &dyn Fn(Vec<usize>) -> Vec<usize>| -> Vec<(ClaimArenaId, i32)> {
        let mut tracker = InMemoryReputationTracker::new();
        let mut claims = vec![claim(0, HONEST_CONF, Some(42), origin_of(0), 1)];
        tracker.set_reputation(&origin_of(0), Reputation::from_bps(HONEST_REP));
        for i in 1..=n {
            // Mix correlated and uncorrelated sources, and vary confidence so
            // that ranking inside a cell is non-trivial.
            let cell = if i % 3 == 0 { None } else { Some(42 + (i % 2)) };
            let conf = HONEST_CONF + (i as i32 % 7) * 13;
            claims.push(claim(i, conf, cell, origin_of(i), 1));
            tracker.set_reputation(&origin_of(i), Reputation::from_bps(HONEST_REP));
        }

        let order = perm((1..=n as usize).collect());
        let edges: Vec<RawEpistemicEdge> = order
            .iter()
            .map(|&i| RawEpistemicEdge {
                from_hash: claims[i].id,
                from_fingerprint: claims[i].fingerprint,
                to_hash: claims[0].id,
                to_fingerprint: claims[0].fingerprint,
                relation: Relation::Supports,
                strength: Reputation::FULL,
            })
            .collect();

        let graph = KnowledgeGraph::build_from_claims(&claims, &edges);
        let base: rustc_hash::FxHashMap<ClaimArenaId, LogOdds> = claims
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let rep = tracker.reputation(&c.origin);
                (i as ClaimArenaId, rep.weight_evidence(c.confidence))
            })
            .collect();

        let out = graph.propagate_trust_full(&base, 5, 5000, &claims, &tracker);
        let mut v: Vec<(ClaimArenaId, i32)> =
            out.into_iter().map(|(k, lo)| (k, lo.value())).collect();
        v.sort();
        v
    };

    let forward = run(&|v| v);
    let reverse = run(&|mut v| {
        v.reverse();
        v
    });
    // Deterministic shuffle (no RNG: fixed stride permutation).
    let strided = run(&|v| {
        let len = v.len();
        let mut out = Vec::with_capacity(len);
        for start in 0..7 {
            let mut i = start;
            while i < len {
                out.push(v[i]);
                i += 7;
            }
        }
        out
    });

    println!("forward target trust: {}", forward[0].1);
    println!("reverse target trust: {}", reverse[0].1);
    println!("strided target trust: {}", strided[0].1);

    assert_eq!(forward, reverse, "edge order must not affect propagation");
    assert_eq!(forward, strided, "edge order must not affect propagation");
    println!("=> bit-identical across all three edge orderings");
}

/// B2. Confirms the structural cause of B1: `aggregate_correlated` has exactly
/// one production call site (the reducer). The belief path computes base trust
/// as `rep.weight_evidence(claim.confidence)` on RAW confidence.
///
/// Consequence: N correlated claims are each classified individually, and each
/// one clears `accept_threshold` on its own, with no cross-claim discounting.
#[test]
fn b2_belief_engine_classifies_correlated_claims_independently() {
    println!("\n=== B2. BeliefEngine on 100 correlated claims ===");
    let engine = LogOddsBeliefEngine::default();
    let mut tracker = InMemoryReputationTracker::new();
    let graph = KnowledgeGraph::new();

    // 100 co-located sensors, same cell, each at 90% self-declared confidence.
    let conf = LogOdds::from_percent(90).value();
    let claims: Vec<Claim> = (0..100u64)
        .map(|i| claim(i, conf, Some(42), origin_of(i), 1))
        .collect();
    for i in 0..100u64 {
        tracker.set_reputation(&origin_of(i), Reputation::FULL);
    }

    let state = engine.compute(&claims, &graph, &tracker);
    println!("accepted : {}", state.accepted.len());
    println!("uncertain: {}", state.uncertain.len());
    println!("rejected : {}", state.rejected.len());

    let reduced = honest_cluster(100, conf, 10000, DISCOUNT);
    println!(
        "reducer would cap the same 100 sources at: {} ({}x a single source)",
        reduced,
        reduced as f64 / honest_cluster(1, conf, 10000, DISCOUNT) as f64
    );

    assert_eq!(
        state.accepted.len(),
        100,
        "every correlated claim is accepted independently of the others"
    );
}

// ════════════════════════════════════════════════════════════════════
// SECTION C — Adversarial control of self-declared metadata.
// ════════════════════════════════════════════════════════════════════

/// C1. Flip threshold: how confident must ONE insider declare itself to
/// outweigh N honest correlated sources? Measured on a single combined
/// aggregate (not by comparing two separately computed numbers).
#[test]
fn c1_flip_threshold_is_flat_in_n() {
    println!("\n=== C1. Insider confidence needed to flip N honest sources ===");
    println!(
        "{:>8}  {:>12}  {:>14}  {:>10}  {:>12}",
        "N", "honest agg", "insider conf", "~percent", "combined"
    );

    let mut thresholds = Vec::new();
    for n in [1usize, 10, 100, 1000, 10_000] {
        let honest = honest_cluster(n, HONEST_CONF, HONEST_REP, DISCOUNT);
        let rep = INSIDER_REP as i64;
        // smallest |conf| such that floor(conf*rep/10000) > honest
        let needed = ((honest + 1) * 10000 + rep - 1) / rep;

        // Build ONE evidence set: honest cluster (cell=42) + insider (cell=None,
        // opposite sign) and verify the combined aggregate actually flips sign.
        let h_rep = Reputation::from_bps(HONEST_REP);
        let mut ev: Vec<(LogOdds, Option<CorrelationCell>, ClaimHash)> = (0..n as u64)
            .map(|i| {
                let mut id = [0u8; 32];
                id[..8].copy_from_slice(&i.to_le_bytes());
                (
                    h_rep.weight_evidence(LogOdds::new(HONEST_CONF)),
                    Some(CorrelationCell(42)),
                    id,
                )
            })
            .collect();
        ev.push((
            Reputation::from_bps(INSIDER_REP).weight_evidence(LogOdds::new(-(needed as i32))),
            None,
            [0xFFu8; 32],
        ));
        let combined = LogOdds::aggregate_correlated(&ev, DISCOUNT);

        println!(
            "{:>8}  {:>12}  {:>14}  {:>9}%  {:>12}",
            n,
            honest,
            needed,
            LogOdds::new(needed as i32).to_percent(),
            combined.value()
        );
        assert!(
            combined.value() < 0,
            "combined belief must flip negative at N={}",
            n
        );
        if n >= 10 {
            thresholds.push(needed);
        }
    }
    assert!(
        thresholds.windows(2).all(|w| w[0] == w[1]),
        "flip threshold must be independent of N, got {:?}",
        thresholds
    );
}

/// C2. CONTROL for C1: same measurement at EQUAL reputation, isolating the
/// discounting geometry from the reputation gap.
#[test]
fn c2_flip_threshold_flat_at_equal_reputation() {
    println!("\n=== C2. Control: insider at EQUAL reputation (6000 bps) ===");
    println!("{:>8}  {:>12}  {:>14}  {:>10}", "N", "honest agg", "insider conf", "~percent");
    let rep = HONEST_REP as i64;
    let mut thresholds = Vec::new();
    for n in [1usize, 10, 100, 1000, 10_000] {
        let honest = honest_cluster(n, HONEST_CONF, HONEST_REP, DISCOUNT);
        let needed = ((honest + 1) * 10000 + rep - 1) / rep;
        println!(
            "{:>8}  {:>12}  {:>14}  {:>9}%",
            n,
            honest,
            needed,
            LogOdds::new(needed as i32).to_percent()
        );
        assert!(
            Reputation::from_bps(HONEST_REP)
                .weight_evidence(LogOdds::new(needed as i32))
                .value() as i64
                > honest
        );
        if n >= 10 {
            thresholds.push(needed);
        }
    }
    assert!(
        thresholds.windows(2).all(|w| w[0] == w[1]),
        "threshold must be flat in N at equal reputation, got {:?}",
        thresholds
    );
}

/// C3. Cell shattering: `correlation_cell` is self-declared and unverified.
/// Identical sources, identical reputation, identical confidence — the only
/// difference is whether they disclose their correlation.
#[test]
fn c3_cell_shattering_penalizes_honest_disclosure() {
    println!("\n=== C3. Cell shattering: same rep, same confidence, N=100 ===");
    let n = 100u64;
    let honest = honest_cluster(n as usize, HONEST_CONF, HONEST_REP, DISCOUNT);

    let rep = Reputation::from_bps(HONEST_REP);
    let shattered_ev: Vec<(LogOdds, Option<CorrelationCell>, ClaimHash)> = (0..n)
        .map(|i| {
            let mut id = [0u8; 32];
            id[..8].copy_from_slice(&i.to_le_bytes());
            (rep.weight_evidence(LogOdds::new(HONEST_CONF)), None, id)
        })
        .collect();
    let shattered = LogOdds::aggregate_correlated(&shattered_ev, DISCOUNT).value() as i64;

    println!("declares cell (honest)   : {}", honest);
    println!("declares None (concealed): {}", shattered);
    println!("advantage                : {:.1}x", shattered as f64 / honest as f64);

    assert!(
        shattered > honest * 10,
        "concealing correlation confers a large unearned advantage"
    );
}

/// C4. Self-declared confidence is unbounded. `LogOdds::new` is a raw
/// constructor with no validation, and L2 transports claims as opaque bytes.
#[test]
fn c4_self_declared_confidence_is_unbounded() {
    println!("\n=== C4. Unbounded self-declared confidence ===");
    let honest_max = honest_cluster(10_000, HONEST_CONF, HONEST_REP, DISCOUNT);
    let absurd = LogOdds::new(i32::MAX);
    let one_insider = Reputation::from_bps(INSIDER_REP)
        .weight_evidence(absurd)
        .value() as i64;

    println!("declared confidence : {}", absurd.value());
    println!("to_percent()        : {}%", absurd.to_percent());
    println!("insider contribution: {}", one_insider);
    println!("10k honest sources  : {}", honest_max);
    println!("ratio               : {:.0}x", one_insider as f64 / honest_max as f64);

    assert!(one_insider > honest_max * 1000);
}

/// C5. FIX VERIFICATION for C4: a claim arriving from the wire with an absurd
/// self-declared confidence must be clamped before it becomes evidence.
///
/// C4 exercises the raw arithmetic API, where large values are legitimate
/// (aggregates grow with evidence). This test exercises the CLAIM path, where
/// the value is attacker-controlled and must be bounded.
#[test]
fn c5_claim_confidence_is_clamped_on_ingestion() {
    println!("\n=== C5. Claim-path confidence clamping ===");

    let hostile = claim(1, i32::MAX, None, origin_of(1), 1);
    println!("wire value            : {}", hostile.confidence.value());
    println!("declared_confidence() : {}", hostile.declared_confidence().value());
    assert_eq!(
        hostile.declared_confidence().value(),
        LogOdds::MAX_DECLARED,
        "absurd confidence must clamp to the declarable maximum"
    );

    let hostile_neg = claim(2, i32::MIN, None, origin_of(2), 1);
    assert_eq!(
        hostile_neg.declared_confidence().value(),
        LogOdds::MIN_DECLARED,
        "absurd negative confidence must clamp too"
    );

    // Honest claims are untouched.
    let honest = claim(3, HONEST_CONF, None, origin_of(3), 1);
    assert_eq!(
        honest.declared_confidence().value(),
        HONEST_CONF,
        "legitimate confidence must pass through unchanged"
    );

    // End-to-end: through the BeliefEngine, the hostile claim can no longer
    // dominate the mesh by orders of magnitude.
    let engine = LogOddsBeliefEngine::default();
    let mut tracker = InMemoryReputationTracker::new();
    tracker.set_reputation(&origin_of(1), Reputation::from_bps(INSIDER_REP));
    let graph = KnowledgeGraph::new();
    let state = engine.compute(std::slice::from_ref(&hostile), &graph, &tracker);

    let bounded = Reputation::from_bps(INSIDER_REP)
        .weight_evidence(hostile.declared_confidence())
        .value() as i64;
    let unbounded = Reputation::from_bps(INSIDER_REP)
        .weight_evidence(hostile.confidence)
        .value() as i64;
    println!("weighted, clamped     : {}", bounded);
    println!("weighted, unclamped   : {}", unbounded);
    println!("reduction             : {:.0}x", unbounded as f64 / bounded as f64);
    println!("belief state          : accepted={}", state.accepted.len());

    assert!(
        bounded * 100_000 < unbounded,
        "clamping must remove several orders of magnitude"
    );
}

/// C6. FIX VERIFICATION for C3, and an honest map of what is still uncovered.
///
/// `effective_correlation_cell()` removes a claim's ability to buy singleton
/// status by withholding metadata. This test measures exactly which concealment
/// shapes are now bounded and which are not, so the residual gap is recorded in
/// code rather than in someone's memory.
#[test]
fn c6_concealment_coverage_map() {
    use aimp_node::semantic_topology::QuantizedEmbedding;

    fn aggregate_claims(claims: &[Claim], tracker: &InMemoryReputationTracker) -> i64 {
        let groups = correlation_groups(claims, 30);
        let ev: Vec<(LogOdds, CorrelationGroup, ClaimHash)> = claims
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let rep = tracker.reputation(&c.origin);
                (
                    rep.weight_evidence(c.declared_confidence()),
                    groups[i],
                    c.id,
                )
            })
            .collect();
        LogOdds::aggregate_hierarchical(&ev, DISCOUNT, UNASSESSED_DISCOUNT_BPS).value() as i64
    }

    let n = 100u64;
    let mut tracker = InMemoryReputationTracker::new();
    for i in 0..n {
        tracker.set_reputation(&origin_of(i), Reputation::from_bps(HONEST_REP));
    }
    tracker.set_reputation(&origin_of(999), Reputation::from_bps(HONEST_REP));

    // Baseline: honest disclosure of a shared cell.
    let disclosed: Vec<Claim> = (0..n)
        .map(|i| claim(i, HONEST_CONF, Some(42), origin_of(i), 1))
        .collect();
    let v_disclosed = aggregate_claims(&disclosed, &tracker);

    // Shape 1: ONE node emits N claims, concealing correlation. Origin fallback.
    let one_node: Vec<Claim> = (0..n)
        .map(|i| claim(i, HONEST_CONF, None, origin_of(999), 1))
        .collect();
    let v_one_node = aggregate_claims(&one_node, &tracker);

    // Shape 2: N distinct nodes, concealing, NO embedding. Origin fallback gives
    // each its own bucket, so this shape remains UNCOVERED by design.
    let many_nodes: Vec<Claim> = (0..n)
        .map(|i| claim(i, HONEST_CONF, None, origin_of(i), 1))
        .collect();
    let v_many_nodes = aggregate_claims(&many_nodes, &tracker);

    // Shape 3: N distinct nodes, concealing, but carrying near-identical
    // embeddings — semantically correlated content. The LSH band should group
    // them despite the concealment.
    let base = QuantizedEmbedding::new([0xDEAD_BEEF_0000_0000, 1, 2, 3]);
    let many_embedded: Vec<Claim> = (0..n)
        .map(|i| {
            // Distinct content (so the consistency rule is satisfied) but nearby
            // embeddings: semantically correlated reports of the same phenomenon.
            let mut c = claim_with_content(
                i,
                HONEST_CONF,
                None,
                origin_of(i),
                1,
                format!("temp-reading-{}", i).as_bytes(),
            );
            // Flip bits in the HIGH end of word 0, i.e. inside the LSH band the
            // previous implementation keyed on. Each claim stays within d<=30 of
            // the base (at most 5 flipped bits) but lands in a DIFFERENT band, so
            // a single-band scheme would fail to group these. Exact clustering
            // must still collapse them into one component.
            let flips = (i % 5) + 1;
            let mut w0 = base.0[0];
            for k in 0..flips {
                w0 ^= 1u64 << (63 - k);
            }
            c.embedding = Some(QuantizedEmbedding::new([
                w0,
                base.0[1],
                base.0[2],
                base.0[3],
            ]));
            c
        })
        .collect();
    let v_many_embedded = aggregate_claims(&many_embedded, &tracker);

    println!("\n=== C6. Concealment coverage (N=100) ===");
    println!("{:<46} {:>10}", "shape", "aggregate");
    println!("{:<46} {:>10}", "honest disclosure (shared cell)", v_disclosed);
    println!("{:<46} {:>10}", "conceal: 1 node, N claims", v_one_node);
    println!("{:<46} {:>10}", "conceal: N nodes, no embedding", v_many_nodes);
    println!("{:<46} {:>10}", "conceal: N nodes, same embedding band", v_many_embedded);

    // COVERED: single-node self-amplification is now bounded like disclosure.
    assert_eq!(
        v_one_node, v_disclosed,
        "one node emitting N claims must gain nothing by concealing"
    );

    // COVERED: semantically correlated content is grouped via the LSH band.
    assert_eq!(
        v_many_embedded, v_disclosed,
        "shared embedding band must be grouped despite concealment"
    );

    // BOUNDED (was 70.3x before level-2 aggregation): N distinct identities with
    // no embedding can no longer manufacture unbounded weight. They converge to
    // 1/(1-0.8) = 5x a single group instead of scaling with N.
    let ratio = v_many_nodes as f64 / v_disclosed as f64;
    println!("\nN distinct origins without embeddings -> {:.1}x disclosure", ratio);
    assert!(
        v_many_nodes > v_disclosed,
        "unassessed independence must still be worth more than known correlation"
    );
    assert!(
        ratio < 5.0,
        "RESIDUAL GAP must stay bounded by the level-2 ceiling, got {:.1}x",
        ratio
    );

    // The gap must NOT grow with N — that is what distinguishes a bounded
    // residual from an exploitable one.
    let bigger: Vec<Claim> = (0..1000u64)
        .map(|i| claim(i, HONEST_CONF, None, origin_of(i), 1))
        .collect();
    let mut big_tracker = InMemoryReputationTracker::new();
    for i in 0..1000u64 {
        big_tracker.set_reputation(&origin_of(i), Reputation::from_bps(HONEST_REP));
    }
    let v_1000 = aggregate_claims(&bigger, &big_tracker);
    println!("same shape at N=1000 -> {} ({:.1}x)", v_1000, v_1000 as f64 / v_disclosed as f64);
    assert!(
        v_1000 < v_many_nodes * 11 / 10,
        "10x more identities must not buy 10x more weight: {} -> {}",
        v_many_nodes,
        v_1000
    );
}

/// C7. What the residual gap actually COSTS.
///
/// C6 measures the gap assuming the attacker already controls N reputable
/// identities. That assumption hides the price. Unknown nodes start at
/// `Reputation::ZERO` and are excluded from aggregation entirely, so the
/// residual gap is not "free evidence" — it is evidence bought at one delegated
/// identity per increment.
///
/// This test prices it: evidence gained per identity acquired, versus the
/// honest-disclosure ceiling.
#[test]
fn c7_residual_gap_is_priced_in_delegated_identities() {
    println!("\n=== C7. Cost of the residual gap ===");
    println!(
        "{:>12}  {:>12}  {:>16}  {:>14}",
        "identities", "undelegated", "delegated", "x disclosure"
    );

    let disclosure_ceiling = {
        let mut t = InMemoryReputationTracker::new();
        for i in 0..100u64 {
            t.set_reputation(&origin_of(i), Reputation::from_bps(HONEST_REP));
        }
        let claims: Vec<Claim> = (0..100u64)
            .map(|i| claim(i, HONEST_CONF, Some(42), origin_of(i), 1))
            .collect();
        let ev: Vec<(LogOdds, Option<CorrelationCell>, ClaimHash)> = claims
            .iter()
            .map(|c| {
                (
                    t.reputation(&c.origin).weight_evidence(c.declared_confidence()),
                    Some(CorrelationCell(c.effective_correlation_cell())),
                    c.id,
                )
            })
            .collect();
        LogOdds::aggregate_correlated(&ev, DISCOUNT).value() as i64
    };
    // (C7 claims carry no embedding, so the context-free fallback is exact here.)

    for k in [1usize, 5, 10, 50, 100] {
        // Attacker controls k identities, all concealing correlation.
        let claims: Vec<Claim> = (0..k as u64)
            .map(|i| claim(i, HONEST_CONF, None, origin_of(i), 1))
            .collect();

        // Case 1: identities are NOT delegated -> reputation 0 -> no weight.
        let empty = InMemoryReputationTracker::new();
        let undelegated: i64 = {
            let ev: Vec<(LogOdds, Option<CorrelationCell>, ClaimHash)> = claims
                .iter()
                .filter(|c| empty.reputation(&c.origin).bps() > 0)
                .map(|c| {
                    (
                        empty.reputation(&c.origin).weight_evidence(c.declared_confidence()),
                        Some(CorrelationCell(c.effective_correlation_cell())),
                        c.id,
                    )
                })
                .collect();
            if ev.is_empty() {
                0
            } else {
                LogOdds::aggregate_correlated(&ev, DISCOUNT).value() as i64
            }
        };

        // Case 2: attacker paid for k delegations.
        let mut paid = InMemoryReputationTracker::new();
        for i in 0..k as u64 {
            paid.set_reputation(&origin_of(i), Reputation::from_bps(HONEST_REP));
        }
        let delegated: i64 = {
            let ev: Vec<(LogOdds, Option<CorrelationCell>, ClaimHash)> = claims
                .iter()
                .map(|c| {
                    (
                        paid.reputation(&c.origin).weight_evidence(c.declared_confidence()),
                        Some(CorrelationCell(c.effective_correlation_cell())),
                        c.id,
                    )
                })
                .collect();
            LogOdds::aggregate_correlated(&ev, DISCOUNT).value() as i64
        };

        println!(
            "{:>12}  {:>12}  {:>16}  {:>13.1}x",
            k,
            undelegated,
            delegated,
            delegated as f64 / disclosure_ceiling as f64
        );

        assert_eq!(
            undelegated, 0,
            "undelegated identities must contribute nothing (Sybil defense)"
        );
    }

    println!(
        "\nhonest disclosure ceiling: {} (flat in N)",
        disclosure_ceiling
    );
    println!("=> the gap is linear in DELEGATED identities, not in claims.");
}

/// C8. Does the embedding itself reopen the hole?
///
/// Level-2 aggregation bounds UNASSESSED independence. Claims carrying an
/// embedding are `assessed` and sum freely — that is deliberate, since a distinct
/// embedding cluster is supposed to substantiate independence.
///
/// But the embedding is ALSO a self-declared, unvalidated wire field. Nothing
/// checks that it corresponds to the claim's content. If K identities can assert
/// identical content while declaring K mutually distant embeddings, they are
/// credited as substantiated independent sources and regain linear scaling —
/// defeating level 2 entirely.
///
/// This test asserts the abuse is DETECTED: claims with identical content and
/// the same `embedding_version` must not be treated as independent.
#[test]
fn c8_inconsistent_embeddings_do_not_buy_independence() {
    use aimp_node::semantic_topology::QuantizedEmbedding;

    let n = 100u64;
    let mut tracker = InMemoryReputationTracker::new();
    for i in 0..n {
        tracker.set_reputation(&origin_of(i), Reputation::from_bps(HONEST_REP));
    }

    fn agg(claims: &[Claim], tracker: &InMemoryReputationTracker) -> i64 {
        let groups = correlation_groups(claims, 30);
        let ev: Vec<(LogOdds, CorrelationGroup, ClaimHash)> = claims
            .iter()
            .enumerate()
            .map(|(i, c)| {
                (
                    tracker
                        .reputation(&c.origin)
                        .weight_evidence(c.declared_confidence()),
                    groups[i],
                    c.id,
                )
            })
            .collect();
        LogOdds::aggregate_hierarchical(&ev, DISCOUNT, UNASSESSED_DISCOUNT_BPS).value() as i64
    }

    // Baseline: honest disclosure of a shared cell.
    let disclosed: Vec<Claim> = (0..n)
        .map(|i| claim(i, HONEST_CONF, Some(42), origin_of(i), 1))
        .collect();
    let v_disclosed = agg(&disclosed, &tracker);

    // Attack: identical content (same fingerprint, same data), but each identity
    // declares a mutually distant embedding to manufacture distinct clusters.
    let attack: Vec<Claim> = (0..n)
        .map(|i| {
            let mut c = claim(i, HONEST_CONF, None, origin_of(i), 1);
            c.embedding = Some(QuantizedEmbedding::new([
                i.wrapping_mul(0x9E37_79B9_7F4A_7C15),
                i.wrapping_mul(0xBF58_476D_1CE4_E5B9),
                i.wrapping_mul(0x94D0_49BB_1331_11EB),
                i.wrapping_mul(0x2545_F491_4F6C_DD1D),
            ]));
            c
        })
        .collect();
    let v_attack = agg(&attack, &tracker);

    println!("\n=== C8. Inconsistent embeddings, identical content (N=100) ===");
    println!("honest disclosure          : {}", v_disclosed);
    println!("forged distinct embeddings : {}", v_attack);
    println!("advantage                  : {:.1}x", v_attack as f64 / v_disclosed as f64);

    assert!(
        v_attack < v_disclosed * 5,
        "identical content with forged distinct embeddings must not buy \
         substantiated independence: {} vs {}",
        v_attack,
        v_disclosed
    );
}

// ════════════════════════════════════════════════════════════════════
// SECTION D — End-to-end through the epoch-aligned pipeline.
// ════════════════════════════════════════════════════════════════════

/// D1. Confirms the asymmetry survives the real reduction pipeline, where
/// bucketing on (epoch, cell) places the honest cluster and the insider into
/// SEPARATE buckets that each produce their own Summary.
#[test]
fn d1_asymmetry_survives_epoch_aligned_reduction() {
    println!("\n=== D1. End-to-end: reduce_epoch_aligned_correlated ===");
    let reducer = ExactMatchReducer;
    let mut tracker = InMemoryReputationTracker::new();

    let n = 100u64;
    let mut claims: Vec<Claim> = Vec::new();
    for i in 0..n {
        claims.push(claim(i, HONEST_CONF, Some(42), origin_of(i), 1));
        tracker.set_reputation(&origin_of(i), Reputation::from_bps(HONEST_REP));
    }
    // Two insider claims sharing cell=None so the bucket is reducible (n>=2).
    let ins_a = origin_of(9001);
    let ins_b = origin_of(9002);
    tracker.set_reputation(&ins_a, Reputation::from_bps(INSIDER_REP));
    tracker.set_reputation(&ins_b, Reputation::from_bps(INSIDER_REP));
    claims.push(claim(9001, -HONEST_CONF, None, ins_a, 1));
    claims.push(claim(9002, -HONEST_CONF, None, ins_b, 1));

    let summaries = reducer.reduce_epoch_aligned_correlated(&claims, 10, Some(&tracker), DISCOUNT);

    println!("summaries produced: {}", summaries.len());
    for s in &summaries {
        let cell = s.correlation_cell.map(|c| c.0);
        if let ClaimKind::Summary { unique_sources, .. } = &s.kind {
            println!(
                "  cell={:?}  sources={}  aggregated={}",
                cell,
                unique_sources,
                s.confidence.value()
            );
        }
    }

    let honest_sum = summaries
        .iter()
        .find(|s| s.correlation_cell == Some(CorrelationCell(42)))
        .expect("honest cluster summary must exist");
    let insider_sum = summaries
        .iter()
        .find(|s| s.correlation_cell.is_none())
        .expect("insider summary must exist");

    println!(
        "\n100 honest sources -> {} | 2 insider sources -> {}",
        honest_sum.confidence.value(),
        insider_sum.confidence.value()
    );
    println!(
        "per-source weight: honest {:.1} vs insider {:.1}",
        honest_sum.confidence.value() as f64 / 100.0,
        insider_sum.confidence.value().abs() as f64 / 2.0
    );

    // Two insiders, undiscounted, already rival 100 discounted honest sources.
    assert!(
        insider_sum.confidence.value().abs() * 2 > honest_sum.confidence.value(),
        "2 undiscounted insiders should rival 100 discounted honest sources"
    );
}

// ════════════════════════════════════════════════════════════════════
// SECTION E — Documentation consistency.
// ════════════════════════════════════════════════════════════════════

/// E1. The doc comment on `to_percent` claims
/// "to_percent(from_percent(x)) == x for all representative values", while the
/// doc comment on `from_percent` claims it "may not equal x". They contradict
/// each other; this test records which one is true.
#[test]
fn e1_percent_roundtrip_consistency() {
    println!("\n=== E1. to_percent(from_percent(x)) round-trip ===");
    let mut broken = Vec::new();
    for pct in [0u8, 5, 10, 20, 30, 40, 50, 60, 70, 80, 90, 95, 99, 100] {
        let lo = LogOdds::from_percent(pct);
        let back = lo.to_percent();
        if back != pct {
            broken.push((pct, lo.value(), back));
        }
        println!("{:>4}% -> {:>7} -> {:>4}%{}", pct, lo.value(), back,
            if back == pct { "" } else { "   <-- MISMATCH" });
    }
    println!("\nmismatches: {:?}", broken);

    // Exactly one mismatch is expected and it is INHERENT, not a bracket bug:
    // `from_percent` maps the whole 95..=99 range onto 2944, so 99 has no distinct
    // log-odds to come back from. Every other canonical value must round-trip.
    //
    // Before the fix, all five of 60/70/80/90/95 also mismatched — the positive
    // brackets were shifted one label up, so user-facing output systematically
    // overstated positive belief while the negative side was correct.
    assert_eq!(
        broken.len(),
        1,
        "only the inherent 95..=99 collapse may fail to round-trip, got {:?}",
        broken
    );
    assert_eq!(broken[0].0, 99, "the sole mismatch must be the 99 collapse");
}

// ════════════════════════════════════════════════════════════════════
// SECTION F — Adversarial re-read of the fixes themselves.
// ════════════════════════════════════════════════════════════════════

/// F1. KEYSPACE COLLISION: can a declared cell impersonate a derived cluster key?
///
/// `aggregate_hierarchical` marks a group assessed only if EVERY member is
/// (`entry.1 &= group.assessed`). Cluster keys are
/// `blake3(domain || embedding_version || anchor_claim_id)` — every input public
/// and reproducible by anyone observing the mesh. If a declared `correlation_cell`
/// is used as a raw u64, an attacker can set it to an honest cluster's key, join
/// that group as an unassessed member, and DOWNGRADE the whole cluster from
/// substantiated independence to the bounded ceiling.
///
/// That is an attack on honest evidence, not on the attacker's own weight.
#[test]
fn f1_declared_cell_cannot_impersonate_a_cluster_key() {
    use aimp_node::semantic_topology::QuantizedEmbedding;

    let mut tracker = InMemoryReputationTracker::new();
    for i in 0..12u64 {
        tracker.set_reputation(&origin_of(i), Reputation::from_bps(HONEST_REP));
    }
    tracker.set_reputation(&origin_of(500), Reputation::from_bps(HONEST_REP));

    // 10 honest sources, distinct content, mutually distant embeddings:
    // substantiated independence, 10 distinct assessed clusters.
    let honest: Vec<Claim> = (0..10u64)
        .map(|i| {
            let mut c = claim_with_content(
                i,
                HONEST_CONF,
                None,
                origin_of(i),
                1,
                format!("obs-{}", i).as_bytes(),
            );
            c.embedding = Some(QuantizedEmbedding::new([
                i.wrapping_mul(0x9E37_79B9_7F4A_7C15),
                i.wrapping_mul(0xBF58_476D_1CE4_E5B9),
                i.wrapping_mul(0x94D0_49BB_1331_11EB),
                i.wrapping_mul(0x2545_F491_4F6C_DD1D),
            ]));
            c
        })
        .collect();

    fn agg(claims: &[Claim], tracker: &InMemoryReputationTracker) -> i64 {
        let groups = correlation_groups(claims, 30);
        let ev: Vec<(LogOdds, CorrelationGroup, ClaimHash)> = claims
            .iter()
            .enumerate()
            .map(|(i, c)| {
                (
                    tracker
                        .reputation(&c.origin)
                        .weight_evidence(c.declared_confidence()),
                    groups[i],
                    c.id,
                )
            })
            .collect();
        LogOdds::aggregate_hierarchical(&ev, DISCOUNT, UNASSESSED_DISCOUNT_BPS).value() as i64
    }

    let baseline = agg(&honest, &tracker);

    // The attacker reads the mesh, recomputes an honest cluster's key, and
    // declares it as its own correlation cell.
    let target_key = correlation_groups(&honest, 30)[0].key;
    let mut attacker = claim(500, HONEST_CONF, Some(target_key), origin_of(500), 1);
    attacker.embedding = None; // unassessed by construction

    let mut poisoned = honest.clone();
    poisoned.push(attacker);
    let after = agg(&poisoned, &tracker);

    let groups_after = correlation_groups(&poisoned, 30);
    let honest0_key = groups_after[0].key;
    let attacker_key = groups_after[poisoned.len() - 1].key;
    let honest0_still_assessed = groups_after[0].assessed;

    println!("\n=== F1. Declared-cell keyspace collision ===");
    println!("10 honest assessed clusters      : {}", baseline);
    println!("after attacker joins cluster key : {}", after);
    println!("honest[0] group key              : {:#018x}", honest0_key);
    println!("attacker  group key              : {:#018x}", attacker_key);
    println!("honest[0] still assessed         : {}", honest0_still_assessed);

    // The real test is not the total — the attacker's own weight can mask a
    // demotion. It is whether an attacker-chosen key can land in the derived
    // cluster keyspace at all.
    assert_ne!(
        attacker_key, honest0_key,
        "DOWNGRADE ATTACK: a declared correlation_cell collided with a derived \
         cluster key, pulling honest substantiated evidence into an unassessed \
         group (total moved {} -> {})",
        baseline, after
    );
    assert!(
        honest0_still_assessed,
        "honest cluster must remain assessed when an unrelated claim is added"
    );
}

/// F2. LEVEL-2 TAIL: is the unassessed ceiling actually flat in K?
///
/// `discount_factor` clamps its exponent at `MAX_DISCOUNT_DEPTH` rather than
/// decaying to zero. At 8000 bps the factor at max depth is small but NONZERO, so
/// every group beyond that rank keeps contributing a constant sliver. If that
/// sliver is not zero, the "bounded" level-2 ceiling actually grows linearly in K
/// with a shallow slope — bounded in appearance, unbounded in fact.
#[test]
fn f2_level2_ceiling_is_flat_in_k() {
    println!("\n=== F2. Level-2 ceiling vs number of manufactured groups ===");
    println!(
        "{:>10}  {:>12}  {:>14}",
        "K groups", "aggregate", "x K=100"
    );

    println!(
        "discount_factor at max depth 30 (8000 bps) = {}",
        discount_factor(30, UNASSESSED_DISCOUNT_BPS)
    );

    // CRITICAL: the per-group magnitude must be large enough that
    // `group_total * factor / 10000` does not truncate to zero. With a small
    // magnitude the tail silently vanishes into integer division and the ceiling
    // LOOKS flat for reasons that do not generalise.
    const PER_GROUP: i32 = 10_000;

    let mut at_100 = 0i64;
    let mut results = Vec::new();
    for k in [100u64, 1_000, 10_000, 100_000] {
        let ev: Vec<(LogOdds, CorrelationGroup, ClaimHash)> = (0..k)
            .map(|i| {
                let mut id = [0u8; 32];
                id[..8].copy_from_slice(&i.to_le_bytes());
                (
                    LogOdds::new(PER_GROUP),
                    CorrelationGroup {
                        key: i,
                        assessed: false,
                    },
                    id,
                )
            })
            .collect();
        let agg = LogOdds::aggregate_hierarchical(&ev, DISCOUNT, UNASSESSED_DISCOUNT_BPS).value()
            as i64;
        if k == 100 {
            at_100 = agg;
        }
        println!("{:>10}  {:>12}  {:>13.2}x", k, agg, agg as f64 / at_100 as f64);
        results.push((k, agg));
    }

    let (_, v_100) = results[0];
    let (_, v_100k) = results[results.len() - 1];
    assert!(
        v_100k < v_100 * 12 / 10,
        "TAIL LEAK: 1000x more manufactured groups bought {}x more weight ({} -> {})",
        v_100k as f64 / v_100 as f64,
        v_100,
        v_100k
    );
}

/// F3. CONVERGENCE: does magnitude-dependent ranking make Pass 1 oscillate?
///
/// Level-1 and level-2 discounting both rank contributions by |magnitude|. Those
/// magnitudes change between fixed-point iterations, so ranks can PERMUTE from one
/// pass to the next. A permuting operator is not monotone, and a non-monotone
/// iteration can cycle instead of converging.
///
/// Consensus is safe either way — the iteration is bounded by `max_iterations`
/// and is deterministic, so every node computes the same value. But a value taken
/// from the middle of a limit cycle is arbitrary, so this must be measured rather
/// than assumed.
///
/// The probe runs the propagation with increasing iteration budgets on a graph
/// built to make ranks contend: mixed cells, mixed assessability, and closely
/// spaced confidences. If the result stabilises as the budget grows, Pass 1
/// reaches a fixed point.
#[test]
fn f3_propagation_reaches_a_fixed_point() {
    use aimp_node::semantic_topology::QuantizedEmbedding;

    let n = 30u64;
    let mut tracker = InMemoryReputationTracker::new();
    let mut claims = vec![claim(0, HONEST_CONF, Some(42), origin_of(0), 1)];
    tracker.set_reputation(&origin_of(0), Reputation::from_bps(HONEST_REP));

    for i in 1..=n {
        // Deliberately adversarial mix: some assessed, some not, some sharing a
        // declared cell, with confidences close enough that ranks can swap.
        let mut c = claim_with_content(
            i,
            HONEST_CONF + (i as i32 % 5),
            if i % 3 == 0 { Some(42) } else { None },
            origin_of(i),
            1,
            format!("obs-{}", i).as_bytes(),
        );
        if i % 2 == 0 {
            c.embedding = Some(QuantizedEmbedding::new([
                i.wrapping_mul(0x9E37_79B9_7F4A_7C15),
                i.wrapping_mul(0xBF58_476D_1CE4_E5B9),
                i.wrapping_mul(0x94D0_49BB_1331_11EB),
                i.wrapping_mul(0x2545_F491_4F6C_DD1D),
            ]));
        }
        claims.push(c);
        tracker.set_reputation(&origin_of(i), Reputation::from_bps(HONEST_REP));
    }

    // Chain the graph so trust must propagate through several hops: each claim
    // supports the previous one, so depth actually matters.
    let mut edges: Vec<RawEpistemicEdge> = Vec::new();
    for i in 1..=n as usize {
        edges.push(RawEpistemicEdge {
            from_hash: claims[i].id,
            from_fingerprint: claims[i].fingerprint,
            to_hash: claims[i - 1].id,
            to_fingerprint: claims[i - 1].fingerprint,
            relation: Relation::Supports,
            strength: Reputation::FULL,
        });
    }

    let graph = KnowledgeGraph::build_from_claims(&claims, &edges);
    let base: rustc_hash::FxHashMap<ClaimArenaId, LogOdds> = claims
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let rep = tracker.reputation(&c.origin);
            (i as ClaimArenaId, rep.weight_evidence(c.declared_confidence()))
        })
        .collect();

    println!("\n=== F3. Fixed-point convergence under permuting ranks ===");
    println!("{:>12}  {:>16}", "iterations", "target trust");

    let mut values = Vec::new();
    for iters in [1u8, 2, 3, 5, 8, 13, 21, 34] {
        let out = graph.propagate_trust_full(&base, iters, 5000, &claims, &tracker);
        let v = out.get(&0).copied().unwrap_or(LogOdds::NEUTRAL).value();
        println!("{:>12}  {:>16}", iters, v);
        values.push(v);
    }

    // Two independent properties, both required to rule out a limit cycle:
    //
    // 1. MONOTONE: trust never decreases as the budget grows. An oscillating
    //    operator would show a value going down at some point.
    // 2. FIXED POINT: the two largest budgets agree exactly.
    //
    // A 30-hop chain needs ~30 passes to saturate, which is why the earlier
    // budgets are still climbing — that is depth, not instability. Production
    // deliberately truncates at 5 passes (bounded depth by design).
    for w in values.windows(2) {
        assert!(
            w[1] >= w[0],
            "non-monotone propagation suggests rank permutation is destabilising \
             the iteration: {:?}",
            values
        );
    }
    let last = values[values.len() - 1];
    let prev = values[values.len() - 2];
    assert_eq!(
        last, prev,
        "Pass 1 did not reach a fixed point at high iteration budgets: {:?}",
        values
    );
    println!("=> monotone, fixed point at {}", last);
}

/// F4. CLAIM-ORDER DETERMINISM — the actual CRDT property.
///
/// B1c permuted EDGE order. This permutes CLAIM order, which is what genuinely
/// differs between nodes in a mesh: claims arrive by gossip, in no agreed
/// sequence. Claim order feeds union-find, the min-`ClaimHash` component anchors,
/// the content/embedding consistency scan, and every group ranking.
///
/// Two nodes holding the same claim SET in different sequences must derive the
/// same belief. If they do not, the mesh forks.
#[test]
fn f4_claim_order_does_not_change_beliefs() {
    use aimp_node::semantic_topology::QuantizedEmbedding;

    let n = 60u64;

    // Build a deliberately hostile mix: assessed and unassessed claims, shared
    // and distinct cells, embeddings that cluster and embeddings that do not,
    // plus a content group with INCONSISTENT embeddings to exercise the
    // fail-closed path.
    let build = || -> (Vec<Claim>, InMemoryReputationTracker) {
        let mut tracker = InMemoryReputationTracker::new();
        let mut claims = Vec::new();
        for i in 0..n {
            let cell = match i % 4 {
                0 => Some(42),
                1 => Some(7),
                _ => None,
            };
            let mut c = claim_with_content(
                i,
                HONEST_CONF + (i as i32 % 11) - 5,
                cell,
                origin_of(i % 17), // deliberate origin collisions
                1 + (i % 3),
                format!("obs-{}", i % 23).as_bytes(), // deliberate content collisions
            );
            if i % 3 != 0 {
                // Some of these will collide on content while differing in
                // embedding -> the consistency rule must fire.
                c.embedding = Some(QuantizedEmbedding::new([
                    (i % 7).wrapping_mul(0x9E37_79B9_7F4A_7C15),
                    i.wrapping_mul(0xBF58_476D_1CE4_E5B9),
                    (i % 5).wrapping_mul(0x94D0_49BB_1331_11EB),
                    i.wrapping_mul(0x2545_F491_4F6C_DD1D),
                ]));
            }
            tracker.set_reputation(&origin_of(i % 17), Reputation::from_bps(HONEST_REP));
            claims.push(c);
        }
        (claims, tracker)
    };

    // Canonical result: map each claim ID to its group key + assessed flag, and
    // to its final belief. Keyed by claim ID so it is permutation-invariant.
    let evaluate = |claims: &[Claim], tracker: &InMemoryReputationTracker| {
        let groups = correlation_groups(claims, 30);

        let mut by_id: Vec<(ClaimHash, u64, bool)> = claims
            .iter()
            .enumerate()
            .map(|(i, c)| (c.id, groups[i].key, groups[i].assessed))
            .collect();
        by_id.sort();

        let ev: Vec<(LogOdds, CorrelationGroup, ClaimHash)> = claims
            .iter()
            .enumerate()
            .map(|(i, c)| {
                (
                    tracker
                        .reputation(&c.origin)
                        .weight_evidence(c.declared_confidence()),
                    groups[i],
                    c.id,
                )
            })
            .collect();
        let agg = LogOdds::aggregate_hierarchical(&ev, DISCOUNT, UNASSESSED_DISCOUNT_BPS).value();

        // Also exercise the propagation path under the same permutation.
        //
        // Edges MUST be anchored to a stable claim ID, never to a position:
        // `claims[0]` names a different claim after a permutation, which would
        // build a different graph and make this test measure nothing.
        let mut target_id = [0u8; 32];
        target_id[..8].copy_from_slice(&0u64.to_le_bytes());
        let target = claims
            .iter()
            .find(|c| c.id == target_id)
            .expect("anchor claim must be present in every permutation");

        let edges: Vec<RawEpistemicEdge> = claims
            .iter()
            .filter(|c| c.id != target_id)
            .map(|c| RawEpistemicEdge {
                from_hash: c.id,
                from_fingerprint: c.fingerprint,
                to_hash: target.id,
                to_fingerprint: target.fingerprint,
                relation: Relation::Supports,
                strength: Reputation::FULL,
            })
            .collect();
        let graph = KnowledgeGraph::build_from_claims(claims, &edges);
        let base: rustc_hash::FxHashMap<ClaimArenaId, LogOdds> = claims
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let rep = tracker.reputation(&c.origin);
                (i as ClaimArenaId, rep.weight_evidence(c.declared_confidence()))
            })
            .collect();
        let prop = graph.propagate_trust_full(&base, 5, 5000, claims, tracker);
        // Key propagation output by claim ID, not arena index, so it survives
        // permutation.
        let mut prop_by_id: Vec<(ClaimHash, i32)> = prop
            .iter()
            .map(|(arena, lo)| (claims[*arena as usize].id, lo.value()))
            .collect();
        prop_by_id.sort();

        (by_id, agg, prop_by_id)
    };

    let (claims, tracker) = build();
    let canonical = evaluate(&claims, &tracker);

    println!("\n=== F4. Claim-order determinism ===");
    println!("claims: {}", claims.len());
    println!("aggregate (canonical order): {}", canonical.1);

    // Several deterministic permutations, no RNG.
    let permutations: Vec<(&str, Box<dyn Fn(Vec<Claim>) -> Vec<Claim>>)> = vec![
        (
            "reversed",
            Box::new(|mut v: Vec<Claim>| {
                v.reverse();
                v
            }),
        ),
        (
            "stride-7",
            Box::new(|v: Vec<Claim>| {
                let len = v.len();
                let mut out = Vec::with_capacity(len);
                for start in 0..7 {
                    let mut i = start;
                    while i < len {
                        out.push(v[i].clone());
                        i += 7;
                    }
                }
                out
            }),
        ),
        (
            "halves-swapped",
            Box::new(|v: Vec<Claim>| {
                let mid = v.len() / 2;
                let mut out = v[mid..].to_vec();
                out.extend_from_slice(&v[..mid]);
                out
            }),
        ),
        (
            "rotated-by-13",
            Box::new(|v: Vec<Claim>| {
                let mut out = v[13..].to_vec();
                out.extend_from_slice(&v[..13]);
                out
            }),
        ),
    ];

    for (name, perm) in permutations {
        let permuted = perm(claims.clone());
        let result = evaluate(&permuted, &tracker);
        println!("  {:<16} aggregate = {}", name, result.1);

        assert_eq!(
            result.0, canonical.0,
            "MESH FORK: claim order '{}' changed group assignment",
            name
        );
        assert_eq!(
            result.1, canonical.1,
            "MESH FORK: claim order '{}' changed the aggregate",
            name
        );
        assert_eq!(
            result.2, canonical.2,
            "MESH FORK: claim order '{}' changed propagated trust",
            name
        );
    }
    println!("=> group assignment, aggregate and propagation all order-invariant");
}

/// F5. COST of the O(N^2) correlation clustering.
///
/// `correlation_groups` does a pairwise Hamming scan, which is O(N^2). That is the
/// same order the v0.4.0 auto-edge generator already pays, and the repository
/// budgets ~50 ms for 10k claims there — but "same order" is not "same constant",
/// and this runs on the belief path, not just at epoch boundaries.
///
/// It is called ONCE per propagation (hoisted out of the fixed-point loop) and
/// once per reducer bucket. This measures it rather than assuming it.
#[test]
fn f5_clustering_cost_is_within_budget() {
    use aimp_node::semantic_topology::QuantizedEmbedding;
    use std::time::Instant;

    println!("\n=== F5. correlation_groups cost ===");
    println!("{:>10}  {:>12}  {:>16}", "N", "elapsed", "per claim");

    let mut last_ms = 0.0f64;
    for n in [100u64, 1_000, 5_000, 10_000] {
        let claims: Vec<Claim> = (0..n)
            .map(|i| {
                let mut c = claim_with_content(
                    i,
                    HONEST_CONF,
                    None,
                    origin_of(i),
                    1,
                    format!("obs-{}", i).as_bytes(),
                );
                c.embedding = Some(QuantizedEmbedding::new([
                    i.wrapping_mul(0x9E37_79B9_7F4A_7C15),
                    i.wrapping_mul(0xBF58_476D_1CE4_E5B9),
                    i.wrapping_mul(0x94D0_49BB_1331_11EB),
                    i.wrapping_mul(0x2545_F491_4F6C_DD1D),
                ]));
                c
            })
            .collect();

        let t0 = Instant::now();
        let groups = correlation_groups(&claims, 30);
        let elapsed = t0.elapsed();
        assert_eq!(groups.len(), claims.len());

        let ms = elapsed.as_secs_f64() * 1000.0;
        last_ms = ms;
        println!(
            "{:>10}  {:>10.2}ms  {:>14.1}ns",
            n,
            ms,
            elapsed.as_nanos() as f64 / n as f64
        );
    }

    // Generous ceiling: debug builds are ~10-50x slower than release, and CI
    // machines vary. This is a smoke alarm for accidental O(N^3), not a
    // performance gate.
    assert!(
        last_ms < 60_000.0,
        "clustering at N=10k took {:.0}ms — far outside the O(N^2) budget",
        last_ms
    );
}
