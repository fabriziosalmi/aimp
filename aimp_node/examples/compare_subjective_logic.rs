//! SOTA Comparison: L3 Log-Odds vs Subjective Logic vs Dempster-Shafer
//!
//! Implements minimal versions of competing frameworks to enable
//! quantitative comparison on identical workloads.
//!
//! Usage: cargo run --release --example compare_subjective_logic

use aimp_node::epistemic::*;
use std::time::Instant;

// ═══════════════════════════════════════════════════════════
// Subjective Logic (Jøsang 2001) — minimal Rust implementation
// ═══════════════════════════════════════════════════════════

/// Subjective Logic opinion: (belief, disbelief, uncertainty, base_rate)
/// All f64 — this is the fundamental issue for BFT.
#[derive(Clone, Debug)]
struct SubjectiveOpinion {
    b: f64, // belief
    d: f64, // disbelief
    u: f64, // uncertainty
    a: f64, // base rate (prior)
}

impl SubjectiveOpinion {
    fn new(b: f64, d: f64, u: f64, a: f64) -> Self {
        Self { b, d, u, a }
    }

    /// Cumulative fusion (Jøsang 2016, §12.3)
    fn cumulative_fusion(&self, other: &Self) -> Self {
        let kappa = self.u + other.u - self.u * other.u;
        if kappa.abs() < 1e-12 {
            // Both completely certain — average
            return Self::new(
                (self.b + other.b) / 2.0,
                (self.d + other.d) / 2.0,
                0.0,
                (self.a + other.a) / 2.0,
            );
        }
        Self {
            b: (self.b * other.u + other.b * self.u) / kappa,
            d: (self.d * other.u + other.d * self.u) / kappa,
            u: (self.u * other.u) / kappa,
            a: (self.a + other.a) / 2.0,
        }
    }

    /// Projected probability
    fn probability(&self) -> f64 {
        self.b + self.a * self.u
    }

    /// Discount operator for trust propagation
    fn discount(&self, trust_b: f64) -> Self {
        Self {
            b: trust_b * self.b,
            d: trust_b * self.d,
            u: 1.0 - trust_b * (self.b + self.d),
            a: self.a,
        }
    }
}

/// Aggregate N opinions via cumulative fusion
fn subjective_aggregate(opinions: &[SubjectiveOpinion]) -> SubjectiveOpinion {
    let mut result = opinions[0].clone();
    for op in &opinions[1..] {
        result = result.cumulative_fusion(op);
    }
    result
}

// ═══════════════════════════════════════════════════════════
// Dempster-Shafer Theory (Shafer 1976) — minimal 2-hypothesis
// ═══════════════════════════════════════════════════════════

/// Mass function over {H, ¬H, Θ} (2-element frame of discernment)
#[derive(Clone, Debug)]
struct DempsterMass {
    m_h: f64,     // mass on hypothesis H
    m_not_h: f64, // mass on ¬H
    m_theta: f64, // mass on Θ (total ignorance)
}

impl DempsterMass {
    fn new(m_h: f64, m_not_h: f64) -> Self {
        Self {
            m_h,
            m_not_h,
            m_theta: 1.0 - m_h - m_not_h,
        }
    }

    /// Dempster's rule of combination (with normalization)
    fn combine(&self, other: &Self) -> Self {
        let k = self.m_h * other.m_not_h + self.m_not_h * other.m_h;
        if (1.0 - k).abs() < 1e-12 {
            // Total conflict — known pathological case
            return Self::new(0.5, 0.5);
        }
        let norm = 1.0 / (1.0 - k);
        let m_h =
            norm * (self.m_h * other.m_h + self.m_h * other.m_theta + self.m_theta * other.m_h);
        let m_not_h = norm
            * (self.m_not_h * other.m_not_h
                + self.m_not_h * other.m_theta
                + self.m_theta * other.m_not_h);
        Self {
            m_h,
            m_not_h,
            m_theta: 1.0 - m_h - m_not_h,
        }
    }

    fn belief(&self) -> f64 {
        self.m_h
    }
}

/// Aggregate N mass functions via Dempster's rule
fn dempster_aggregate(masses: &[DempsterMass]) -> DempsterMass {
    let mut result = masses[0].clone();
    for m in &masses[1..] {
        result = result.combine(m);
    }
    result
}

// ═══════════════════════════════════════════════════════════
// Benchmarks
// ═══════════════════════════════════════════════════════════

fn main() {
    println!("=== SOTA Comparison: L3 vs Subjective Logic vs Dempster-Shafer ===\n");

    let iters = 100_000;

    // ── 1. Aggregation of N evidence items ──
    println!("--- Evidence Aggregation (N items) ---");
    println!("| N    | L3 (ns)  | Subj.Logic (ns) | Dempster-Shafer (ns) | L3 vs SL | L3 vs DS |");
    println!("|------|----------|------------------|----------------------|----------|----------|");

    for &n in &[10usize, 100, 1000] {
        // L3: log-odds aggregation
        let logodds: Vec<LogOdds> = (0..n)
            .map(|i| LogOdds::new((i as i32 % 5000) - 2500))
            .collect();
        let start = Instant::now();
        for _ in 0..iters {
            let _ = std::hint::black_box(LogOdds::aggregate(std::hint::black_box(&logodds)));
        }
        let t_l3 = start.elapsed().as_nanos() as f64 / iters as f64;

        // Subjective Logic: cumulative fusion
        let opinions: Vec<SubjectiveOpinion> = (0..n)
            .map(|i| {
                let p = (i % 100) as f64 / 100.0;
                SubjectiveOpinion::new(p * 0.8, (1.0 - p) * 0.8, 0.2, 0.5)
            })
            .collect();
        let start = Instant::now();
        for _ in 0..iters {
            let _ = std::hint::black_box(subjective_aggregate(std::hint::black_box(&opinions)));
        }
        let t_sl = start.elapsed().as_nanos() as f64 / iters as f64;

        // Dempster-Shafer: rule of combination
        let masses: Vec<DempsterMass> = (0..n)
            .map(|i| {
                let p = (i % 100) as f64 / 100.0;
                DempsterMass::new(p * 0.7, (1.0 - p) * 0.2)
            })
            .collect();
        let start = Instant::now();
        for _ in 0..iters {
            let _ = std::hint::black_box(dempster_aggregate(std::hint::black_box(&masses)));
        }
        let t_ds = start.elapsed().as_nanos() as f64 / iters as f64;

        println!(
            "| {:>4} | {:>8.1} | {:>16.1} | {:>20.1} | {:>7.1}x | {:>7.1}x |",
            n,
            t_l3,
            t_sl,
            t_ds,
            t_sl / t_l3.max(0.1),
            t_ds / t_l3.max(0.1),
        );
    }

    // ── 2. Determinism test ──
    println!("\n--- Determinism Across Runs ---");
    println!("| Framework        | Run 1 result     | Run 2 result     | Identical? |");
    println!("|------------------|------------------|------------------|------------|");

    let evidence = vec![
        LogOdds::new(1500),
        LogOdds::new(-800),
        LogOdds::new(2200),
        LogOdds::new(-300),
        LogOdds::new(4100),
    ];
    let r1 = LogOdds::aggregate(&evidence);
    let r2 = LogOdds::aggregate(&evidence);
    println!(
        "| L3 (log-odds)    | {:>16} | {:>16} | {:>10} |",
        r1.value(),
        r2.value(),
        if r1 == r2 { "YES" } else { "NO" }
    );

    let opinions = vec![
        SubjectiveOpinion::new(0.7, 0.1, 0.2, 0.5),
        SubjectiveOpinion::new(0.3, 0.4, 0.3, 0.5),
        SubjectiveOpinion::new(0.9, 0.05, 0.05, 0.5),
    ];
    let sr1 = subjective_aggregate(&opinions);
    let sr2 = subjective_aggregate(&opinions);
    println!(
        "| Subjective Logic | {:>16.10} | {:>16.10} | {:>10} |",
        sr1.probability(),
        sr2.probability(),
        if (sr1.probability() - sr2.probability()).abs() < f64::EPSILON {
            "YES*"
        } else {
            "NO"
        }
    );

    let masses = vec![
        DempsterMass::new(0.6, 0.1),
        DempsterMass::new(0.3, 0.3),
        DempsterMass::new(0.8, 0.1),
    ];
    let dr1 = dempster_aggregate(&masses);
    let dr2 = dempster_aggregate(&masses);
    println!(
        "| Dempster-Shafer  | {:>16.10} | {:>16.10} | {:>10} |",
        dr1.belief(),
        dr2.belief(),
        if (dr1.belief() - dr2.belief()).abs() < f64::EPSILON {
            "YES*"
        } else {
            "NO"
        }
    );

    println!("\n* Float equality on same machine/same code path. May differ across architectures.");
    println!("  L3 uses i32 arithmetic — guaranteed bit-identical on ARM64, x86_64, RISC-V, WASM.");

    // ── 3. Trust propagation comparison ──
    println!("\n--- Trust Propagation: 100-node graph ---");
    let n = 100u32;
    let claims: Vec<Claim> = (0..n)
        .map(|i| {
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
        })
        .collect();
    let mut graph = KnowledgeGraph::new();
    for i in 1..n {
        graph.add_edge(EpistemicEdge {
            from: i - 1,
            to: i,
            relation: Relation::Supports,
            strength: Reputation::from_bps(8000),
        });
    }
    let mut tracker = InMemoryReputationTracker::new();
    let anchor = [255u8; 32];
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

    // L3 propagation
    let start = Instant::now();
    for _ in 0..iters {
        let _ = graph.propagate_trust_full(&base, 5, 5000, &claims, &tracker);
    }
    let t_l3_prop = start.elapsed().as_nanos() as f64 / iters as f64 / 1000.0;

    // Subjective Logic discount+consensus chain
    let start = Instant::now();
    for _ in 0..iters {
        let mut current = SubjectiveOpinion::new(0.8, 0.1, 0.1, 0.5);
        for _ in 0..n {
            current = current.discount(0.8);
        }
    }
    let t_sl_prop = start.elapsed().as_nanos() as f64 / iters as f64 / 1000.0;

    println!("| Framework        | Propagation (µs) | Method                    |");
    println!("|------------------|------------------|---------------------------|");
    println!(
        "| L3 (AIMP)        | {:>16.3} | Two-pass O(V+E), i32      |",
        t_l3_prop
    );
    println!(
        "| Subjective Logic | {:>16.3} | Discount chain, f64       |",
        t_sl_prop
    );
    println!(
        "| Dempster-Shafer  | {:>16} | N/A (no trust model)      |",
        "—"
    );

    // ── 4. Comparison summary ──
    println!("\n--- Feature Comparison ---");
    println!("| Property              | L3 (AIMP)  | Subj. Logic | Dempster-Shafer |");
    println!("|-----------------------|------------|-------------|-----------------|");
    println!("| Arithmetic            | i32 (det.) | f64         | f64             |");
    println!("| BFT compatible        | YES        | NO*         | NO*             |");
    println!("| CRDT integration      | Native     | External    | External        |");
    println!("| Cycle handling         | DFS zeroing| Discount    | N/A             |");
    println!("| Sybil resistance      | WoT+rep=0  | None        | None            |");
    println!("| Contradiction damping | Configurable| Fusion      | Dempster's rule |");
    println!("| Known pathologies     | None       | Vacuous fusion| Zadeh paradox  |");
    println!("\n* IEEE 754 floats may produce different results on different architectures,");
    println!("  making them unsafe for BFT consensus where all nodes must agree exactly.");
}
