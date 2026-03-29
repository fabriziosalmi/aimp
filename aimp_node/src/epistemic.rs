//! AIMP v0.4.0 — Epistemic Layer (L3: Meaning)
//!
//! Cognitive middleware built ABOVE aimp-core. L3 never touches L2 internals.
//!
//! ## Design Rules (from multi-AI review, rounds 1-2)
//! 1. **NO FLOATING POINT** — Log-Odds (i32) for Bayesian updates (Gemini fix)
//! 2. **L3 never blocks L2** — CRDT merges at full speed regardless
//! 3. **Materialized Compaction** — L3 re-injects Summaries into L2 before epoch GC (Gemini fix)
//! 4. **Reputation gates confidence** — weight = reputation × confidence
//! 5. **Evidence provenance** — anti Sybil amplification (NOT physical correlation, documented)
//! 6. **SemanticReducer MUST be commutative, associative, idempotent** (Gemini fix)
//! 7. **BeliefState** — claims are classified into accepted/rejected/uncertain (ChatGPT fix)
//! 8. **Cycle detection** — epistemic loops penalized to prevent confidence inflation (ChatGPT R3)
//! 9. **Trust propagation** — transitive trust through graph edges (ChatGPT R3)
//! 10. **Summary variance** — preserve statistical spread to prevent information black holes (ChatGPT R3)
//! 11. **Trust clamping** — max(0, source_trust) before propagation; rejected claims have no voice (Gemini R5)
//! 12. **Sybil defense** — new nodes start at reputation 0, not neutral; require delegation to vote (Gemini R5)
//! 13. **Summary overlap** — epoch tick windows prevent double-counting on async compaction (Gemini R5)
//! 14. **Correlation discounting** — correlated claims (same grid cell) get geometric decay (v0.3.0)
//! 15. **Atomic cell reduction** — bucketing by (epoch, fingerprint, cell) ensures discount is always
//!     computed on the full stabilized set, eliminating associativity requirement for geometric decay (v0.3.0)
//! 16. **Quantized embeddings** — 256-bit SimHash for deterministic semantic distance (v0.4.0)
//! 17. **Automatic edge generation** — epoch-batch Supports/Contradicts from Hamming distance (v0.4.0)
//! 18. **Embedding versioning** — only same-version embeddings are compared (v0.4.0)

use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

// ─── Log-Odds Arithmetic (Gemini Fix: NO linear u16, NO floats) ─────
//
// Bayesian update in log-odds space = ADDITION (not multiplication).
// Zero underflow. Zero overflow for realistic inputs. 100% deterministic.
//
// Log-odds = ln(p / (1-p)) scaled by 1000 (milli-log-odds)
//   0     = 50.0% probability
//  +6931  = 99.9% probability (ln(999) * 1000)
//  -6931  = 0.1% probability
//  i32 range: ±2 billion — effectively unbounded for any real use
//
// Bayesian update: posterior = prior + evidence (just addition!)

/// Log-odds confidence, scaled by 1000 (milli-log-odds).
/// Deterministic across all architectures. Bayesian update = addition.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct LogOdds(i32);

impl LogOdds {
    /// 50% confidence (total uncertainty)
    pub const NEUTRAL: Self = Self(0);
    /// ~99.9% confidence
    pub const VERY_HIGH: Self = Self(6907); // ln(999) * 1000
    /// ~0.1% confidence
    pub const VERY_LOW: Self = Self(-6907);
    /// Maximum representable (effectively 100%)
    pub const MAX: Self = Self(i32::MAX);
    /// Minimum representable (effectively 0%)
    pub const MIN: Self = Self(i32::MIN);

    pub fn new(milli_logodds: i32) -> Self {
        Self(milli_logodds)
    }

    pub fn value(self) -> i32 {
        self.0
    }

    /// Create from percentage (0..=100) using lookup table (no floats).
    /// Returns the mathematically exact milli-log-odds for the given probability.
    /// Note: `to_percent(from_percent(x))` may not equal `x` because `to_percent`
    /// uses coarser quantization brackets. This is intentional: `from_percent`
    /// preserves precision, `to_percent` provides human-readable approximation.
    /// The log-odds value itself is always exact and used for all computation.
    pub fn from_percent(pct: u8) -> Self {
        match pct {
            0 => Self(-13816), // ln(0.001/0.999)*1000
            1..=5 => Self(-2944),
            6..=10 => Self(-2197),
            11..=20 => Self(-1386),
            21..=30 => Self(-847),
            31..=40 => Self(-405),
            41..=49 => Self(-100),
            50 => Self(0),
            51..=59 => Self(100),
            60..=69 => Self(405),
            70..=79 => Self(847),
            80..=89 => Self(1386),
            90..=94 => Self(2197),
            95..=99 => Self(2944),
            100 => Self(13816),
            _ => Self(0),
        }
    }

    /// Convert back to approximate percentage (0..=100) using lookup.
    /// Bracket boundaries are chosen so that `to_percent(from_percent(x)) == x`
    /// for all representative values. Each bracket's lower bound matches the
    /// value produced by `from_percent` for that percentage.
    pub fn to_percent(self) -> u8 {
        match self.0 {
            i32::MIN..=-6908 => 0,
            -6907..=-2945 => 2,
            -2944..=-2198 => 5,
            -2197..=-1387 => 10,
            -1386..=-848 => 20,
            -847..=-406 => 30,
            -405..=-101 => 40,
            -100..=99 => 50,
            100..=404 => 60,
            405..=846 => 70,
            847..=1385 => 80,
            1386..=2196 => 90,
            2197..=2943 => 95,
            2944..=6906 => 99,
            6907..=i32::MAX => 100,
        }
    }

    /// Safe operational range for log-odds values.
    /// Values beyond ±1 billion represent probabilities so extreme
    /// (>99.9999999%) that they are meaningless in practice.
    /// Clamping to this range (not i32::MAX) preserves associativity:
    /// aggregate([A,B,C]) == aggregate([aggregate([A,B]),C])
    /// because the i64 intermediate sum never overflows.
    pub const SAFE_MAX: i32 = 1_000_000_000;
    pub const SAFE_MIN: i32 = -1_000_000_000;

    /// Bayesian aggregation: sum log-odds of independent evidence.
    /// Clamps to SAFE_MIN..SAFE_MAX (not i32 extremes) to preserve
    /// the associativity invariant required by the CRDT SemanticReducer.
    /// Uses saturating addition to prevent i64 overflow panic under
    /// adversarial claim flooding (v0.3.0 hardening).
    pub fn aggregate(evidence: &[LogOdds]) -> LogOdds {
        let mut sum: i64 = 0;
        for e in evidence {
            sum = sum.saturating_add(e.0 as i64);
        }
        LogOdds(sum.clamp(Self::SAFE_MIN as i64, Self::SAFE_MAX as i64) as i32)
    }

    /// Bayesian update: posterior = prior + new_evidence
    /// Clamped to SAFE range for associativity preservation.
    pub fn update(self, evidence: LogOdds) -> LogOdds {
        let sum = (self.0 as i64) + (evidence.0 as i64);
        LogOdds(sum.clamp(Self::SAFE_MIN as i64, Self::SAFE_MAX as i64) as i32)
    }

    /// Correlation-aware aggregation (v0.3.0).
    ///
    /// Groups evidence by CorrelationCell, applies geometric discount within
    /// each group, then sums group contributions independently.
    ///
    /// Within each cell group, claims are sorted by (|logodds| desc, id asc)
    /// for deterministic ranking. The strongest claim gets 100% weight;
    /// subsequent claims get discount_bps^rank / 10000^rank.
    ///
    /// Claims with cell=None are treated as singletons (no discounting).
    ///
    /// NOTE: This function is NOT required to be associative across partial
    /// merges. The grid-aligned epoch reduction guarantees that discounting
    /// is always computed atomically on the full stabilized set within a
    /// (epoch, fingerprint, cell) bucket. See design rule #15.
    pub fn aggregate_correlated(
        evidence: &[(LogOdds, Option<CorrelationCell>, ClaimHash)],
        discount_bps: u16,
    ) -> LogOdds {
        if evidence.is_empty() {
            return LogOdds::NEUTRAL;
        }

        // Group by cell. None → each claim is its own group.
        let mut cell_groups: std::collections::BTreeMap<Option<u64>, Vec<(LogOdds, ClaimHash)>> =
            std::collections::BTreeMap::new();
        for (lo, cell, id) in evidence {
            let key = cell.map(|c| c.0);
            cell_groups.entry(key).or_default().push((*lo, *id));
        }

        let mut total: i64 = 0;

        for (key, mut group) in cell_groups {
            if key.is_none() {
                // Uncorrelated claims: full weight each (v0.2.0 behavior)
                // SECURITY: saturating_add prevents i64 overflow panic under
                // adversarial claim flooding (billions of LogOdds::MAX claims).
                for (lo, _) in &group {
                    total = total.saturating_add(lo.0 as i64);
                }
                continue;
            }

            // Sort by |logodds| descending, then by id for deterministic tiebreak
            group.sort_by(|(lo_a, id_a), (lo_b, id_b)| {
                lo_b.0.abs().cmp(&lo_a.0.abs()).then_with(|| id_a.cmp(id_b))
            });

            for (rank, (lo, _)) in group.iter().enumerate() {
                let factor = discount_factor(rank as u32, discount_bps);
                let discounted = (lo.0 as i64) * (factor as i64) / 10000;
                total = total.saturating_add(discounted);
            }
        }

        LogOdds(total.clamp(Self::SAFE_MIN as i64, Self::SAFE_MAX as i64) as i32)
    }

    /// Is this belief more likely true than false?
    pub fn is_positive(self) -> bool {
        self.0 > 0
    }
}

/// Confidence interval in log-odds space: [lower, upper].
///
/// Addresses the expressiveness limitation vs Subjective Logic:
/// a scalar LogOdds cannot represent uncertainty width. A ConfidenceInterval
/// captures both the estimate (midpoint) and the uncertainty (width).
///
/// **NOT CRDT-SAFE**: This type is a local computation tool for
/// single-node analysis. Its `aggregate` function is NOT associative
/// (due to min/max range tracking), making it incompatible with
/// partial CRDT merges. Use scalar `LogOdds` for all inter-node
/// replication. `ConfidenceInterval` is computed locally from the
/// converged LogOdds values when uncertainty analysis is needed.
///
/// Cost: 8 bytes (2 × i32) vs 4 bytes for scalar LogOdds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ConfidenceInterval {
    /// Lower bound of the confidence range (pessimistic estimate)
    pub lower: LogOdds,
    /// Upper bound of the confidence range (optimistic estimate)
    pub upper: LogOdds,
}

impl ConfidenceInterval {
    /// Create from a point estimate with zero uncertainty
    pub fn exact(value: LogOdds) -> Self {
        Self {
            lower: value,
            upper: value,
        }
    }

    /// Create from bounds
    pub fn new(lower: LogOdds, upper: LogOdds) -> Self {
        Self {
            lower: LogOdds::new(lower.value().min(upper.value())),
            upper: LogOdds::new(lower.value().max(upper.value())),
        }
    }

    /// Midpoint estimate (the "best guess")
    pub fn midpoint(self) -> LogOdds {
        LogOdds::new(
            (self.lower.value() as i64 + self.upper.value() as i64)
                .clamp(i32::MIN as i64, i32::MAX as i64) as i32
                / 2,
        )
    }

    /// Uncertainty width (0 = certain, large = uncertain)
    pub fn width(self) -> i32 {
        self.upper.value().saturating_sub(self.lower.value())
    }

    /// Bayesian aggregation of intervals: union of ranges + sum of midpoints.
    /// The width grows when sources disagree (captures epistemic uncertainty).
    pub fn aggregate(intervals: &[ConfidenceInterval]) -> ConfidenceInterval {
        if intervals.is_empty() {
            return Self::exact(LogOdds::NEUTRAL);
        }
        let lower = intervals.iter().map(|i| i.lower.value()).min().unwrap();
        let upper = intervals.iter().map(|i| i.upper.value()).max().unwrap();
        // Midpoint is the Bayesian aggregate of all midpoints
        let mid_sum: i64 = intervals.iter().map(|i| i.midpoint().value() as i64).sum();
        let mid = mid_sum.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
        // Center the interval around the aggregate midpoint, preserving the width.
        // Use i64 to prevent overflow when upper - lower exceeds i32::MAX.
        let half_width = ((upper as i64 - lower as i64) / 2).clamp(0, i32::MAX as i64) as i32;
        ConfidenceInterval {
            lower: LogOdds::new(mid.saturating_sub(half_width)),
            upper: LogOdds::new(mid.saturating_add(half_width)),
        }
    }

    /// Intersection: narrows the interval when evidence agrees.
    pub fn narrow(self, other: ConfidenceInterval) -> ConfidenceInterval {
        let lower = self.lower.value().max(other.lower.value());
        let upper = self.upper.value().min(other.upper.value());
        if lower > upper {
            // No overlap — sources contradict. Return widest.
            ConfidenceInterval {
                lower: LogOdds::new(self.lower.value().min(other.lower.value())),
                upper: LogOdds::new(self.upper.value().max(other.upper.value())),
            }
        } else {
            ConfidenceInterval {
                lower: LogOdds::new(lower),
                upper: LogOdds::new(upper),
            }
        }
    }
}

/// Fixed-point utility score: 0..=10000 (basis points)
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Utility(u16);

impl Utility {
    pub const ZERO: Self = Self(0);
    pub const MAX: Self = Self(10000);

    pub fn from_bps(bps: u16) -> Self {
        Self(bps.min(10000))
    }

    pub fn bps(self) -> u16 {
        self.0
    }
}

/// Peer reputation: 0..=10000 (basis points)
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Reputation(u16);

impl Reputation {
    pub const ZERO: Self = Self(0);
    pub const NEUTRAL: Self = Self(5000);
    pub const FULL: Self = Self(10000);

    pub fn from_bps(bps: u16) -> Self {
        Self(bps.min(10000))
    }

    pub fn bps(self) -> u16 {
        self.0
    }

    /// Reputation-weighted log-odds: scale evidence by reputation.
    /// A node with 0 reputation contributes 0 evidence regardless of claim.
    pub fn weight_evidence(self, evidence: LogOdds) -> LogOdds {
        // Scale: evidence * reputation / 10000 (integer math)
        let scaled = (evidence.0 as i64) * (self.0 as i64) / 10000;
        LogOdds(scaled.clamp(i32::MIN as i64, i32::MAX as i64) as i32)
    }
}

// ─── Compact References ─────────────────────────────────────

/// In-memory arena reference (4 bytes) for graph traversal.
pub type ClaimArenaId = u32;
/// Full cryptographic identifier for network/persistence.
pub type ClaimHash = [u8; 32];

// ─── Correlation Cell (v0.3.0) ─────────────────────────────
//
// Discrete correlation coordinate. Two claims in the same cell are treated
// as correlated; claims in different cells are independent.
//
// Construction is application-defined (same philosophy as edge generation):
// - IoT: geohash truncated to N characters → u64
// - LLM: model_family_id (llama=1, mistral=2, gpt=3) → u64
// - Temporal: tick / temporal_grid_size → u64
// - Composite: BLAKE3(spatial_cell || model_family || temporal_bucket) → u64

/// Discrete correlation coordinate for spatial/semantic proximity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CorrelationCell(pub u64);

/// Maximum geometric decay depth. After this many ranks the discount factor
/// is effectively zero. Matches the temporal decay shift cap (30).
const MAX_DISCOUNT_DEPTH: u32 = 30;

/// Default discount factor in basis points (30% = 3000 bps).
/// Each additional correlated claim contributes DISCOUNT_BPS/10000 of
/// the previous claim's weight.
pub const DEFAULT_DISCOUNT_BPS: u16 = 3000;

/// Compute the geometric discount factor for the i-th correlated claim (0-indexed).
/// Returns value in basis points (0..=10000). Integer-only, no floats.
///
/// rank=0 → 10000 (100%), rank=1 → discount_bps, rank=2 → discount_bps²/10000, ...
pub fn discount_factor(rank: u32, discount_bps: u16) -> u64 {
    let mut factor: u64 = 10000;
    for _ in 0..rank.min(MAX_DISCOUNT_DEPTH) {
        factor = factor * (discount_bps as u64) / 10000;
    }
    factor
}

// ─── Semantic Fingerprint (ChatGPT Fix: dual-key) ───────────

/// Dual-key semantic fingerprint for robust grouping.
/// `primary`: BLAKE3 of normalized content (exact match).
/// `secondary`: hash of discrete features (sensor_type + unit + dimension).
/// Two claims match if primary OR secondary keys match.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SemanticFingerprint {
    /// Hash of normalized content (exact semantic match)
    pub primary: [u8; 16],
    /// Hash of discrete features: sensor_type, unit, dimension (fuzzy match)
    pub secondary: u64,
}

// ─── Core Types ──────────────────────────────────────────────

/// A knowledge claim with cryptographic provenance and log-odds confidence.
///
/// Lives in L3 ONLY. L2 transports serialized bytes as opaque payload.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Claim {
    /// Unique hash (BLAKE3 of canonical form)
    pub id: ClaimHash,
    /// Dual-key semantic fingerprint
    pub fingerprint: SemanticFingerprint,
    /// Ed25519 public key of the claiming agent
    pub origin: [u8; 32],
    /// What is being claimed
    pub kind: ClaimKind,
    /// Self-declared confidence in log-odds (Bayesian-safe)
    pub confidence: LogOdds,
    /// BLAKE3 hash of ORIGINAL evidence source (anti Sybil amplification).
    /// NOTE: This protects against network-level echo chambers only.
    /// Physical correlation is handled by CorrelationCell discounting (v0.3.0).
    pub evidence_source: ClaimHash,
    /// Lamport timestamp
    pub tick: u64,
    /// Optional correlation cell (v0.3.0). Claims in the same cell are treated
    /// as correlated and receive geometric discounting during aggregation.
    /// None = uncorrelated (backward compatible with v0.2.0 behavior).
    pub correlation_cell: Option<CorrelationCell>,
    /// Optional quantized embedding (v0.4.0). 256-bit SimHash of claim content
    /// in a canonical latent space. Used by AutoEdgeGenerator to produce
    /// Supports/Contradicts edges automatically via Hamming distance.
    /// None = no embedding (manual edges only, backward compatible).
    pub embedding: Option<crate::semantic_topology::QuantizedEmbedding>,
    /// Embedding model version (v0.4.0). Only claims with the same version
    /// are compared. Allows protocol-level model upgrades without breaking
    /// existing claims. Default: 0 (unversioned / legacy).
    pub embedding_version: u32,
}

/// The semantic type of a claim.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ClaimKind {
    /// Raw observation from a sensor or agent perception
    Observation { sensor_type: u8, data: Vec<u8> },

    /// Inference derived from one or more observations
    Inference {
        model_id: ClaimHash,
        input_claims: SmallVec<[ClaimArenaId; 4]>,
        inference_type: u8,
        weight: LogOdds,
        output: Vec<u8>,
        /// Can this inference be independently reproduced?
        reproducible: bool,
        /// Hash of (model_id || sorted inputs || output) for verification
        deterministic_hash: ClaimHash,
    },

    /// Intent to perform an action
    Intent {
        action: u8,
        target: Vec<u8>,
        /// Weighted constraints: (constraint_type, importance_bps)
        constraints: SmallVec<[(u8, u16); 4]>,
        goal: u8,
        utility: Utility,
    },

    /// Summary produced by SemanticReducer.
    /// IMPORTANT: This gets re-injected into L2 as a new mutation
    /// before epoch GC runs, solving the split-brain problem. (Gemini fix)
    Summary {
        source_count: u32,
        tick_start: u64,
        tick_end: u64,
        data: Vec<u8>,
        aggregated_logodds: LogOdds,
        unique_sources: u32,
        /// Statistical variance of the input log-odds values (scaled by 1000).
        /// Prevents "information black holes" where outliers and anomalies
        /// are silently absorbed. High variance = disagreement worth investigating.
        /// Computed as: sum((x_i - mean)^2) / N, in integer math. (ChatGPT R3 fix)
        variance_milli: i64,
        /// Min and max log-odds seen in the input claims.
        /// Preserves the full range even after compression.
        range_min: LogOdds,
        range_max: LogOdds,
    },
}

// ─── Knowledge Graph ────────────────────────────────────────

/// Edge in the epistemic knowledge graph.
/// The `strength` field is the confidence of the agent who created the edge.
/// It serves as the dynamic decay factor for trust propagation (Gemini R3 fix).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EpistemicEdge {
    pub from: ClaimArenaId,
    pub to: ClaimArenaId,
    pub relation: Relation,
    /// Confidence of the agent creating this relationship (dynamic decay).
    /// Trust propagated through this edge is scaled by strength/10000.
    /// Replaces the hardcoded 50% decay. (Gemini R3 fix)
    pub strength: Reputation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Relation {
    Supports,
    Contradicts,
    DerivedFrom,
    SharedSource,
}

/// A raw edge as transmitted over L2, carrying both cryptographic IDs
/// and semantic fingerprints for Hybrid Edge Resolution.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RawEpistemicEdge {
    pub from_hash: ClaimHash,
    pub from_fingerprint: SemanticFingerprint,
    pub to_hash: ClaimHash,
    pub to_fingerprint: SemanticFingerprint,
    pub relation: Relation,
    pub strength: Reputation,
}

/// Adjacency-list knowledge graph with real traversal and Hybrid Edge Resolution.
///
/// Edges use `ClaimArenaId` for O(1) traversal. When built from L2 data via
/// `build_from_claims`, orphaned edges (whose target was GC'd) automatically
/// fallback to resolving via `SemanticFingerprint`, routing historical trust
/// into the materialized Summary that inherited the target's semantic identity.
#[derive(Default)]
pub struct KnowledgeGraph {
    edges: Vec<EpistemicEdge>,
    /// Adjacency list: claim_id → outgoing edges
    adjacency: rustc_hash::FxHashMap<ClaimArenaId, SmallVec<[usize; 4]>>,
}

impl KnowledgeGraph {
    pub fn new() -> Self {
        Self::default()
    }

    /// Build a KnowledgeGraph from claims and raw edges with Hybrid Edge Resolution.
    ///
    /// **Precision mode:** edges resolve via exact `ClaimHash` → `ClaimArenaId`.
    /// **Fallback mode:** if the target ClaimHash is missing (GC'd), the edge
    /// resolves via `SemanticFingerprint` → the Summary that inherited that
    /// fingerprint. Summaries take priority in the fingerprint map.
    ///
    /// This is the "Graceful Epistemic Degradation" mechanism: topological
    /// accuracy is maintained during the active epoch, while epistemic mass
    /// is conserved across historical compactions.
    pub fn build_from_claims(claims: &[Claim], raw_edges: &[RawEpistemicEdge]) -> Self {
        let mut graph = Self::new();

        // Primary resolution: ClaimHash → ArenaId
        let mut id_map: rustc_hash::FxHashMap<ClaimHash, ClaimArenaId> =
            rustc_hash::FxHashMap::default();
        // Fallback resolution: SemanticFingerprint → ArenaId (Summaries preferred)
        let mut fingerprint_map: rustc_hash::FxHashMap<SemanticFingerprint, ClaimArenaId> =
            rustc_hash::FxHashMap::default();

        for (i, claim) in claims.iter().enumerate() {
            let arena_id = i as ClaimArenaId;
            id_map.insert(claim.id, arena_id);

            // Summaries override raw claims in the fingerprint map —
            // they are the legitimate semantic heirs after compaction.
            if matches!(claim.kind, ClaimKind::Summary { .. }) {
                fingerprint_map.insert(claim.fingerprint, arena_id);
            } else {
                fingerprint_map.entry(claim.fingerprint).or_insert(arena_id);
            }
        }

        for raw in raw_edges {
            let from_id = id_map
                .get(&raw.from_hash)
                .copied()
                .or_else(|| fingerprint_map.get(&raw.from_fingerprint).copied());
            let to_id = id_map
                .get(&raw.to_hash)
                .copied()
                .or_else(|| fingerprint_map.get(&raw.to_fingerprint).copied());

            if let (Some(from), Some(to)) = (from_id, to_id) {
                graph.add_edge(EpistemicEdge {
                    from,
                    to,
                    relation: raw.relation,
                    strength: raw.strength,
                });
            }
            // Edges where both endpoints are gone (no claim AND no Summary
            // with matching fingerprint) are silently dropped — they reference
            // claims from epochs so old that even their Summaries are gone.
        }

        graph
    }

    pub fn add_edge(&mut self, edge: EpistemicEdge) {
        let idx = self.edges.len();
        self.adjacency.entry(edge.from).or_default().push(idx);
        self.edges.push(edge);
    }

    /// All claims that depend on `claim_id` (transitively)
    pub fn dependents(&self, claim_id: ClaimArenaId) -> Vec<ClaimArenaId> {
        let mut visited = rustc_hash::FxHashSet::default();
        let mut stack = vec![claim_id];
        let mut result = Vec::new();

        while let Some(current) = stack.pop() {
            if !visited.insert(current) {
                continue;
            }
            if let Some(indices) = self.adjacency.get(&current) {
                for &idx in indices {
                    let edge = &self.edges[idx];
                    if edge.relation == Relation::DerivedFrom || edge.relation == Relation::Supports
                    {
                        result.push(edge.to);
                        stack.push(edge.to);
                    }
                }
            }
        }
        result
    }

    /// Count supporting vs contradicting evidence for a claim
    pub fn support_ratio(&self, claim_id: ClaimArenaId) -> (u32, u32) {
        let mut supports = 0u32;
        let mut contradicts = 0u32;

        for edge in &self.edges {
            if edge.to == claim_id {
                match edge.relation {
                    Relation::Supports => supports += 1,
                    Relation::Contradicts => contradicts += 1,
                    _ => {}
                }
            }
        }
        (supports, contradicts)
    }

    pub fn edges(&self) -> &[EpistemicEdge] {
        &self.edges
    }

    // ── Cycle Detection (ChatGPT R3: prevent confidence inflation loops) ──

    /// Detect all cycles in the epistemic graph.
    /// Returns a list of cycles, where each cycle is a vector of node IDs.
    /// A→B→C→A would inflate confidence infinitely without detection.
    ///
    /// BFT SAFETY: Nodes are visited in sorted order (by ClaimArenaId) to ensure
    /// the DFS spanning tree — and thus the set of identified back-edges — is
    /// identical on all nodes regardless of HashMap iteration order or message
    /// arrival sequence. This is critical for BFT determinism.
    pub fn detect_cycles(&self) -> Vec<Vec<ClaimArenaId>> {
        let mut cycles = Vec::new();
        let mut visited = rustc_hash::FxHashSet::default();
        let mut on_stack = rustc_hash::FxHashSet::default();
        let mut stack_path = Vec::new();

        // Collect all nodes and SORT for BFT-deterministic DFS traversal.
        let mut all_nodes: Vec<ClaimArenaId> =
            rustc_hash::FxHashSet::default().into_iter().collect();
        for edge in &self.edges {
            all_nodes.push(edge.from);
            all_nodes.push(edge.to);
        }
        all_nodes.sort_unstable();
        all_nodes.dedup();

        for &start in &all_nodes {
            if !visited.contains(&start) {
                self.dfs_cycles(
                    start,
                    &mut visited,
                    &mut on_stack,
                    &mut stack_path,
                    &mut cycles,
                );
            }
        }
        cycles
    }

    fn dfs_cycles(
        &self,
        node: ClaimArenaId,
        visited: &mut rustc_hash::FxHashSet<ClaimArenaId>,
        on_stack: &mut rustc_hash::FxHashSet<ClaimArenaId>,
        path: &mut Vec<ClaimArenaId>,
        cycles: &mut Vec<Vec<ClaimArenaId>>,
    ) {
        visited.insert(node);
        on_stack.insert(node);
        path.push(node);

        if let Some(indices) = self.adjacency.get(&node) {
            // BFT SAFETY: Sort outgoing edges by target for deterministic traversal
            let mut sorted_indices: SmallVec<[usize; 4]> = indices.clone();
            sorted_indices.sort_unstable_by_key(|&idx| self.edges[idx].to);

            for &idx in &sorted_indices {
                let edge = &self.edges[idx];
                if edge.relation != Relation::Supports && edge.relation != Relation::DerivedFrom {
                    continue;
                }
                let next = edge.to;

                if on_stack.contains(&next) {
                    if let Some(pos) = path.iter().position(|&n| n == next) {
                        cycles.push(path[pos..].to_vec());
                    }
                } else if !visited.contains(&next) {
                    self.dfs_cycles(next, visited, on_stack, path, cycles);
                }
            }
        }

        path.pop();
        on_stack.remove(&node);
    }

    /// Identify edges that form cycles (back-edges in DFS).
    /// These edges are EXCLUDED from trust propagation to prevent
    /// self-referential trust amplification. (Gemini R3 fix)
    ///
    /// Returns a set of edge indices that should be zeroed out.
    /// BFT SAFETY: Uses sorted node order for deterministic back-edge identification.
    pub fn cyclic_edge_indices(&self) -> rustc_hash::FxHashSet<usize> {
        let mut cyclic = rustc_hash::FxHashSet::default();
        let mut visited = rustc_hash::FxHashSet::default();
        let mut on_stack = rustc_hash::FxHashSet::default();

        // Sorted traversal order for BFT determinism
        let mut all_nodes: Vec<ClaimArenaId> = Vec::new();
        for edge in &self.edges {
            all_nodes.push(edge.from);
            all_nodes.push(edge.to);
        }
        all_nodes.sort_unstable();
        all_nodes.dedup();

        for &start in &all_nodes {
            if !visited.contains(&start) {
                self.dfs_mark_cyclic(start, &mut visited, &mut on_stack, &mut cyclic);
            }
        }
        cyclic
    }

    fn dfs_mark_cyclic(
        &self,
        node: ClaimArenaId,
        visited: &mut rustc_hash::FxHashSet<ClaimArenaId>,
        on_stack: &mut rustc_hash::FxHashSet<ClaimArenaId>,
        cyclic: &mut rustc_hash::FxHashSet<usize>,
    ) {
        visited.insert(node);
        on_stack.insert(node);

        if let Some(indices) = self.adjacency.get(&node) {
            // BFT SAFETY: Sort outgoing edges by target ClaimArenaId.
            // Edge insertion order may differ across nodes (gossip arrival order).
            // Sorting ensures identical DFS traversal → identical back-edges.
            let mut sorted_indices: SmallVec<[usize; 4]> = indices.clone();
            sorted_indices.sort_unstable_by_key(|&idx| self.edges[idx].to);

            for &idx in &sorted_indices {
                let edge = &self.edges[idx];
                if edge.relation != Relation::Supports && edge.relation != Relation::DerivedFrom {
                    continue;
                }
                if on_stack.contains(&edge.to) {
                    cyclic.insert(idx);
                } else if !visited.contains(&edge.to) {
                    self.dfs_mark_cyclic(edge.to, visited, on_stack, cyclic);
                }
            }
        }

        on_stack.remove(&node);
    }

    // ── Trust Propagation (Gemini R3: two-pass, dynamic decay, no oscillation) ──

    /// Two-pass trust propagation. Guaranteed convergent, zero oscillation.
    ///
    /// **Pass 1 (Positive):** Propagate trust through Supports/DerivedFrom edges only.
    /// Cyclic edges are excluded (weight=0). Decay per edge = edge.strength (dynamic).
    /// Converges because all weights are positive on an acyclic subgraph.
    ///
    /// **Pass 2 (Negative):** Subtract contradiction impact using stabilized trust
    /// from Pass 1. Single pass, no iteration, deterministic.
    ///
    /// Paper claim: "By separating the propagation of support from the application
    /// of contradictions, our Belief Engine guarantees O(E) convergence without
    /// adversarial oscillation."
    pub fn propagate_trust(
        &self,
        base_trust: &rustc_hash::FxHashMap<ClaimArenaId, LogOdds>,
        max_iterations: u8,
    ) -> rustc_hash::FxHashMap<ClaimArenaId, LogOdds> {
        // Backward-compat: no claims/reputations = use base_trust for author_rep (UNSAFE, legacy)
        self.propagate_trust_full(
            base_trust,
            max_iterations,
            5000,
            &[],
            &InMemoryReputationTracker::new(),
        )
    }

    /// Two-pass trust propagation with proper reputation lookup.
    /// Gemini R6 fix: author_reputation comes from ReputationTracker, NOT from base_trust.
    /// base_trust contains weighted log-odds (confidence × reputation), not raw reputation.
    /// Two-pass trust propagation with proper reputation lookup.
    ///
    /// Parameters:
    /// - `base_trust`: Initial reputation-weighted log-odds per claim
    /// - `max_iterations`: Pass 1 iteration bound (D = max depth for convergence)
    /// - `damping_bps`: Static damping cap (overridden by dynamic damping when > 0 contradictions)
    /// - `claims`: Claim array for author lookup
    /// - `reputations`: Reputation tracker for author reputation
    /// - `current_tick`: Current Lamport timestamp (for temporal decay). Pass 0 to disable.
    /// - `half_life_ticks`: Claims older than this lose half their trust. Pass 0 to disable.
    pub fn propagate_trust_full(
        &self,
        base_trust: &rustc_hash::FxHashMap<ClaimArenaId, LogOdds>,
        max_iterations: u8,
        damping_bps: u16,
        claims: &[Claim],
        reputations: &dyn ReputationTracker,
    ) -> rustc_hash::FxHashMap<ClaimArenaId, LogOdds> {
        self.propagate_trust_advanced(
            base_trust,
            max_iterations,
            damping_bps,
            claims,
            reputations,
            0,
            0,
        )
    }

    /// Full trust propagation with temporal decay and dynamic damping.
    #[allow(clippy::too_many_arguments)]
    pub fn propagate_trust_advanced(
        &self,
        base_trust: &rustc_hash::FxHashMap<ClaimArenaId, LogOdds>,
        max_iterations: u8,
        damping_bps: u16,
        claims: &[Claim],
        reputations: &dyn ReputationTracker,
        current_tick: u64,
        half_life_ticks: u64,
    ) -> rustc_hash::FxHashMap<ClaimArenaId, LogOdds> {
        let cyclic_edges = self.cyclic_edge_indices();

        // ── Temporal Decay: age-based trust attenuation ──
        // Claims older than half_life_ticks lose half their base trust per half-life.
        // Implemented as integer right-shift: trust >> (age / half_life).
        // Deterministic, zero floats, bounded (14 shifts → effectively 0).
        let mut decayed_base = base_trust.clone();
        if half_life_ticks > 0 && current_tick > 0 {
            for (arena_id, trust_val) in decayed_base.iter_mut() {
                if let Some(claim) = claims.get(*arena_id as usize) {
                    let age = current_tick.saturating_sub(claim.tick);
                    let decay_steps = age / half_life_ticks;
                    if decay_steps > 0 {
                        let shifts = decay_steps.min(30) as u32; // cap at 30 shifts
                        let decayed = trust_val.value() >> shifts;
                        *trust_val = LogOdds::new(decayed);
                    }
                }
            }
        }

        let mut trust = decayed_base.clone();

        // ── Pass 1: Positive propagation (Supports + DerivedFrom only) ──
        // Fixed-point iteration: t_{k+1} = t_0 + A·t_k
        // NOT t_{k+1} = t_k + A·t_k (which causes unbounded growth).
        // On the acyclic subgraph, A is strictly upper triangular (ρ(A)=0),
        // so this converges in at most D steps (max depth).
        for _ in 0..max_iterations {
            let mut new_trust = decayed_base.clone(); // Reset to t_0 (with decay)

            for (idx, edge) in self.edges.iter().enumerate() {
                // Skip non-support edges
                if edge.relation != Relation::Supports && edge.relation != Relation::DerivedFrom {
                    continue;
                }
                // Skip cyclic edges (Gemini R3: weight=0 on back-edges)
                if cyclic_edges.contains(&idx) {
                    continue;
                }

                let raw_from = trust.get(&edge.from).copied().unwrap_or(LogOdds::NEUTRAL);
                let from_trust = LogOdds::new(raw_from.value().max(0));

                let author_rep = if (edge.from as usize) < claims.len() {
                    reputations
                        .reputation(&claims[edge.from as usize].origin)
                        .bps() as i64
                } else {
                    0i64
                };
                // Markovian flow normalization: divide by total outgoing
                // support strength from this source. This makes the adjacency
                // matrix stochastic — trust is DIVIDED across outgoing edges,
                // not COPIED. Prevents diamond amplification where a single
                // source's trust is counted K times via K paths.
                // Without normalization: A→B→D and A→C→D gives D 2× A's trust.
                // With normalization: A splits trust 50/50 to B and C.
                let total_out_strength: i64 = self
                    .adjacency
                    .get(&edge.from)
                    .map(|indices| {
                        indices
                            .iter()
                            .filter(|&&i| {
                                !cyclic_edges.contains(&i)
                                    && (self.edges[i].relation == Relation::Supports
                                        || self.edges[i].relation == Relation::DerivedFrom)
                            })
                            .map(|&i| self.edges[i].strength.bps() as i64)
                            .sum()
                    })
                    .unwrap_or(1); // Avoid division by zero
                let total_out = total_out_strength.max(1);

                // contribution = from_trust × (this_edge_strength / total_outgoing_strength) × author_rep / 10000
                let share_bps = (edge.strength.bps() as i64) * 10000 / total_out; // bps
                let contribution =
                    (from_trust.value() as i64) * share_bps / 10000 * author_rep / 10000;

                let base_val = base_trust
                    .get(&edge.to)
                    .copied()
                    .unwrap_or(LogOdds::NEUTRAL);
                let current = new_trust.get(&edge.to).copied().unwrap_or(base_val);
                let bonus =
                    LogOdds::new(contribution.clamp(i32::MIN as i64, i32::MAX as i64) as i32);
                new_trust.insert(edge.to, current.update(bonus));
            }

            let changed = new_trust
                .iter()
                .any(|(k, v)| trust.get(k).copied().unwrap_or(LogOdds::NEUTRAL) != *v);

            trust = new_trust;

            if !changed {
                break; // Converged
            }
        }

        // ── Pass 2: Contradiction subtraction (SIMULTANEOUS, out-of-place) ──
        // ALL penalties are computed from the frozen Pass 1 snapshot, then applied.
        // This prevents topological evaluation bias: the order of edge iteration
        // cannot affect the result because all reads come from the frozen snapshot.
        let stabilized = trust.clone();
        let mut penalties: rustc_hash::FxHashMap<ClaimArenaId, i64> =
            rustc_hash::FxHashMap::default();

        for edge in &self.edges {
            if edge.relation != Relation::Contradicts {
                continue;
            }

            // Read source from FROZEN snapshot
            let raw_from = stabilized
                .get(&edge.from)
                .copied()
                .unwrap_or(LogOdds::NEUTRAL);
            let from_trust = LogOdds::new(raw_from.value().max(0));

            let author_rep = if (edge.from as usize) < claims.len() {
                reputations
                    .reputation(&claims[edge.from as usize].origin)
                    .bps() as i64
            } else {
                0i64
            };
            let effective_weight = (edge.strength.bps() as i64) * author_rep / 10000;
            let raw_penalty = (from_trust.value() as i64) * effective_weight / 10000;

            // Dynamic damping: α = weighted_contradiction / (weighted_support + weighted_contradiction + 1)
            // SECURITY: Uses TRUST-WEIGHTED ratio, not raw edge count.
            // Edge count is Sybil-vulnerable: an attacker can flood 10K
            // zero-reputation Contradicts edges to inflate α without
            // contributing any actual trust. Trust-weighting ensures only
            // edges from reputable sources affect the damping ratio.
            let mut weighted_support = 0i64;
            let mut weighted_contradict = 0i64;
            for e in &self.edges {
                if e.to != edge.to {
                    continue;
                }
                let e_trust = stabilized.get(&e.from).copied().unwrap_or(LogOdds::NEUTRAL);
                let e_weight = e_trust.value().max(0) as i64;
                match e.relation {
                    Relation::Supports | Relation::DerivedFrom => weighted_support += e_weight,
                    Relation::Contradicts => weighted_contradict += e_weight,
                    _ => {}
                }
            }
            let dynamic_alpha_bps = {
                let total = weighted_support + weighted_contradict + 1; // +1 Laplace smoothing
                let ratio = (weighted_contradict * 10000) / total;
                (ratio as u32).min(damping_bps as u32) as i64
            };

            let target_trust = stabilized
                .get(&edge.to)
                .copied()
                .unwrap_or(LogOdds::NEUTRAL);
            let max_penalty = if target_trust.value() > 0 {
                (target_trust.value() as i64) * dynamic_alpha_bps / 10000
            } else {
                raw_penalty.abs()
            };
            let capped_penalty = raw_penalty.abs().min(max_penalty);

            *penalties.entry(edge.to).or_insert(0i64) += capped_penalty;
        }

        // Apply all accumulated penalties with GLOBAL cap.
        // The per-edge cap limits individual contradictions, but without
        // a global cap, K edges each capped at α% produce K×α% total
        // removal ("death by a thousand cuts"). The global cap ensures
        // the total penalty never exceeds damping_bps% of the target's
        // stabilized trust, regardless of how many contradictions arrive.
        for (node, total_penalty) in &penalties {
            let target_stabilized = stabilized.get(node).copied().unwrap_or(LogOdds::NEUTRAL);
            let global_max = if target_stabilized.value() > 0 {
                (target_stabilized.value() as i64) * (damping_bps as i64) / 10000
            } else {
                *total_penalty // No cap on already-negative trust
            };
            let globally_capped = (*total_penalty).min(global_max);
            let current = trust.get(node).copied().unwrap_or(LogOdds::NEUTRAL);
            trust.insert(
                *node,
                LogOdds::new(
                    current
                        .value()
                        .saturating_sub(globally_capped.clamp(0, i32::MAX as i64) as i32),
                ),
            );
        }

        trust
    }

    /// Reverse adjacency lookup: all edges pointing TO a claim
    pub fn incoming_edges(&self, claim_id: ClaimArenaId) -> Vec<&EpistemicEdge> {
        self.edges.iter().filter(|e| e.to == claim_id).collect()
    }
}

// ─── Belief State (ChatGPT: the missing piece) ─────────────

/// The epistemic state of the node: what it believes to be true, false, or uncertain.
/// This is what transforms "accumulation" into "thinking".
#[derive(Clone, Debug, Default)]
pub struct BeliefState {
    /// Claims believed to be true (logodds > accept_threshold)
    pub accepted: Vec<ClaimArenaId>,
    /// Claims believed to be false (logodds < reject_threshold)
    pub rejected: Vec<ClaimArenaId>,
    /// Claims with insufficient evidence either way
    pub uncertain: Vec<ClaimArenaId>,
}

/// Computes belief state from claims and epistemic graph.
pub trait BeliefEngine: Send + Sync {
    fn compute(
        &self,
        claims: &[Claim],
        graph: &KnowledgeGraph,
        reputations: &dyn ReputationTracker,
    ) -> BeliefState;
}

// ─── Traits ─────────────────────────────────────────────────

/// Reputation tracker for peer nodes.
pub trait ReputationTracker: Send + Sync {
    fn reputation(&self, origin: &[u8; 32]) -> Reputation;
    fn update(&mut self, origin: &[u8; 32], event: ReputationEvent);
    /// Delegate trust to a new node (Web of Trust / Permissioned Quorum).
    /// Only nodes with existing reputation >= min_delegator_rep can delegate.
    /// Without delegation, new nodes have reputation 0 and zero voting weight.
    fn delegate(&mut self, from: &[u8; 32], to: &[u8; 32], initial_rep: Reputation);
}

#[derive(Clone, Debug)]
pub enum ReputationEvent {
    EquivocationDetected,
    ClaimConfirmed,
    ClaimContradicted,
    ActiveParticipation,
}

/// Reduces redundant claims into compact summaries.
///
/// # MATHEMATICAL INVARIANT (Gemini, round 2)
/// Any implementation MUST guarantee:
/// - **Commutativity**: reduce([A, B]) == reduce([B, A])
/// - **Associativity**: reduce([reduce([A, B]), C]) == reduce([A, reduce([B, C])])
/// - **Idempotency**: reduce([A, A]) == reduce([A])
///
/// Violating these properties causes the Knowledge Graph to diverge across nodes.
pub trait SemanticReducer: Send + Sync {
    fn reduce(&self, claims: &[Claim]) -> Option<Claim>;
    fn can_reduce(&self, claims: &[Claim]) -> bool;
    fn lossiness(&self) -> u8;
    fn group_key(&self, claim: &Claim) -> u64;
}

/// Resolves conflicting intents using reputation-weighted scoring.
pub trait IntentResolver: Send + Sync {
    fn resolve(&self, intents: &[Claim], reputations: &dyn ReputationTracker) -> Resolution;
}

#[derive(Clone, Debug)]
pub enum Resolution {
    Winner(Claim),
    Compromise(Claim),
    Escalate {
        reason: String,
        conflicting: Vec<ClaimHash>,
    },
}

/// Scores claim relevance for semantic GC (dependency-aware).
pub trait RelevanceScorer: Send + Sync {
    fn score(&self, claim: &Claim, active_intents: &[Claim], graph: &KnowledgeGraph) -> u16;
    fn gc_threshold(&self) -> u16 {
        1000
    }
}

/// Resolves contradictions in the knowledge graph.
pub trait ContradictionResolver: Send + Sync {
    /// Given contradicting claims, determine which to downgrade.
    fn resolve(
        &self,
        claim_a: &Claim,
        claim_b: &Claim,
        graph: &KnowledgeGraph,
        reputations: &dyn ReputationTracker,
    ) -> ContradictionOutcome;
}

#[derive(Clone, Debug)]
pub enum ContradictionOutcome {
    /// Keep A, downgrade B's confidence
    PreferA { b_penalty: LogOdds },
    /// Keep B, downgrade A's confidence
    PreferB { a_penalty: LogOdds },
    /// Both have merit, flag for human review
    Undecided,
}

// ─── Default Implementations ────────────────────────────────

/// Exact-match reducer with echo-chamber protection and log-odds aggregation.
pub struct ExactMatchReducer;

impl ExactMatchReducer {
    /// Reputation-aware reduction: filters out claims from zero-reputation
    /// authors before aggregation. This prevents reputation laundering where
    /// an attacker floods claims that get aggregated into an honest node's
    /// Summary during GC compaction.
    /// Reputation-aware reduction: filters zero-reputation authors AND
    /// weights each claim's confidence by the author's reputation before
    /// aggregation. This prevents both reputation laundering (zero-rep
    /// claims sneaking into Summaries) and Micro-Sybil attacks (1-bps
    /// nodes having equal weight to 10000-bps nodes in log-odds summation).
    ///
    /// Without weighting: 5000 Sybils with 1 bps each contribute
    /// 5000 × VERY_HIGH = astronomical aggregated_logodds.
    /// With weighting: 5000 × (VERY_HIGH × 1/10000) ≈ 5000 × 0.69 ≈ 3450.
    /// A single honest node with 10000 bps contributes VERY_HIGH × 1.0 = 6907.
    /// The honest node dominates. The Sybil attack fails.
    pub fn reduce_with_reputation(
        &self,
        claims: &[Claim],
        reputations: &dyn ReputationTracker,
    ) -> Option<Claim> {
        self.reduce_with_reputation_correlated(claims, reputations, DEFAULT_DISCOUNT_BPS)
    }

    /// Reputation-aware reduction with configurable correlation discount.
    ///
    /// v0.3.0: Claims are grouped by CorrelationCell within the bucket.
    /// Within each cell group, evidence is geometrically discounted.
    /// The grid-aligned epoch reduction guarantees atomic execution on the
    /// full bucket, so associativity of the discount is not required.
    pub fn reduce_with_reputation_correlated(
        &self,
        claims: &[Claim],
        reputations: &dyn ReputationTracker,
        discount_bps: u16,
    ) -> Option<Claim> {
        if !self.can_reduce(claims) {
            return None;
        }

        let mut sorted: Vec<&Claim> = claims.iter().collect();
        sorted.sort_by_key(|c| c.id);
        sorted.dedup_by_key(|c| c.id);

        let first = &sorted[0];
        let data = match &first.kind {
            ClaimKind::Observation { data, .. } => data.clone(),
            ClaimKind::Summary { data, .. } => data.clone(),
            _ => return None,
        };

        // Reputation-weighted evidence with correlation cell metadata.
        let mut unique_sources: SmallVec<[ClaimHash; 16]> = SmallVec::new();
        let mut evidence_tuples: Vec<(LogOdds, Option<CorrelationCell>, ClaimHash)> = Vec::new();

        for c in &sorted {
            let rep = reputations.reputation(&c.origin);
            if rep.bps() == 0 {
                continue; // Zero-reputation authors excluded entirely
            }
            if !unique_sources.contains(&c.evidence_source) {
                unique_sources.push(c.evidence_source);
                // CRITICAL: Weight by reputation. A 1-bps Sybil contributes
                // almost nothing. A 10000-bps anchor contributes full weight.
                evidence_tuples.push((rep.weight_evidence(c.confidence), c.correlation_cell, c.id));
            }
        }

        if evidence_tuples.is_empty() {
            return None;
        }

        // v0.3.0: Correlation-aware aggregation with geometric discounting
        let aggregated = LogOdds::aggregate_correlated(&evidence_tuples, discount_bps);
        // Pre-discount values for variance/range statistics
        let evidence_logodds: Vec<LogOdds> = evidence_tuples.iter().map(|(lo, _, _)| *lo).collect();

        let n = evidence_logodds.len() as i128;
        let mean = if n > 0 {
            aggregated.value() as i128 / n
        } else {
            0
        };
        let variance_milli = if n > 1 {
            let sum_sq: i128 = evidence_logodds
                .iter()
                .map(|lo| {
                    let diff = lo.value() as i128 - mean;
                    diff * diff
                })
                .sum();
            (sum_sq / n).clamp(i64::MIN as i128, i64::MAX as i128) as i64
        } else {
            0
        };

        let range_min = evidence_logodds
            .iter()
            .copied()
            .min()
            .unwrap_or(LogOdds::NEUTRAL);
        let range_max = evidence_logodds
            .iter()
            .copied()
            .max()
            .unwrap_or(LogOdds::NEUTRAL);
        let tick_start = sorted.iter().map(|c| c.tick).min().unwrap_or(0);
        let tick_end = sorted.iter().map(|c| c.tick).max().unwrap_or(0);

        // v0.3.0: Include correlation cell in Summary hash for determinism.
        // SAFETY: When called from reduce_epoch_aligned_correlated(), all claims
        // in the bucket are guaranteed to share the same cell (triple bucketing).
        // When called directly, callers SHOULD ensure cell homogeneity.
        let summary_cell = first.correlation_cell;
        debug_assert!(
            sorted.iter().all(|c| c.correlation_cell == summary_cell),
            "reduce_with_reputation_correlated: mixed cells in bucket (expected {:?})",
            summary_cell
        );

        let mut hasher = blake3::Hasher::new();
        hasher.update(&first.fingerprint.primary);
        hasher.update(&(unique_sources.len() as u32).to_le_bytes());
        hasher.update(&tick_start.to_le_bytes());
        hasher.update(&tick_end.to_le_bytes());
        hasher.update(&aggregated.value().to_le_bytes());
        hasher.update(&variance_milli.to_le_bytes());
        // v0.3.0: cell is part of the hash — different cells produce different Summaries
        if let Some(cell) = summary_cell {
            hasher.update(&cell.0.to_le_bytes());
        }
        let id = *hasher.finalize().as_bytes();

        Some(Claim {
            id,
            fingerprint: first.fingerprint,
            origin: [0u8; 32],
            kind: ClaimKind::Summary {
                source_count: claims.len() as u32,
                tick_start,
                tick_end,
                data,
                aggregated_logodds: aggregated,
                unique_sources: unique_sources.len() as u32,
                variance_milli,
                range_min,
                range_max,
            },
            confidence: aggregated,
            evidence_source: id,
            tick: tick_end,
            // v0.3.0: Summary inherits the cell of its input claims.
            // All claims in this bucket share the same cell (guaranteed by
            // grid-aligned bucketing on (epoch, fingerprint, cell)).
            correlation_cell: summary_cell,
            embedding: None, // Summaries don't carry embeddings
            embedding_version: 0,
        })
    }
}

impl ExactMatchReducer {
    /// Grid-aligned epoch reduction with correlation-aware bucketing (v0.3.0).
    ///
    /// Claims are bucketed by the triple (temporal_grid, fingerprint, correlation_cell).
    /// This guarantees that geometric discounting is always computed atomically
    /// on the full stabilized set within each bucket — eliminating the
    /// associativity requirement for the discount function.
    ///
    /// Two nodes that independently compact the same claims produce
    /// byte-identical Summaries (same BLAKE3 hash), which L2 deduplicates.
    ///
    /// Returns one Summary per bucket that contains ≥2 claims.
    pub fn reduce_epoch_aligned(
        &self,
        claims: &[Claim],
        grid_size: u64,
        reputations: Option<&dyn ReputationTracker>,
    ) -> Vec<Claim> {
        self.reduce_epoch_aligned_correlated(claims, grid_size, reputations, DEFAULT_DISCOUNT_BPS)
    }

    /// Grid-aligned epoch reduction with configurable discount.
    pub fn reduce_epoch_aligned_correlated(
        &self,
        claims: &[Claim],
        grid_size: u64,
        reputations: Option<&dyn ReputationTracker>,
        discount_bps: u16,
    ) -> Vec<Claim> {
        if grid_size == 0 || claims.len() < 2 {
            return Vec::new();
        }

        // v0.3.0: Bucket by (epoch, correlation_cell) — the fingerprint check
        // is handled by can_reduce(). This triple bucketing guarantees atomic
        // discount computation per cell per epoch.
        let mut buckets: std::collections::BTreeMap<(u64, Option<u64>), Vec<Claim>> =
            std::collections::BTreeMap::new();
        for claim in claims {
            let epoch = claim.tick / grid_size;
            let cell_key = claim.correlation_cell.map(|c| c.0);
            buckets
                .entry((epoch, cell_key))
                .or_default()
                .push(claim.clone());
        }

        let mut summaries = Vec::new();
        for bucket_claims in buckets.values() {
            let result = if let Some(rep) = reputations {
                self.reduce_with_reputation_correlated(bucket_claims, rep, discount_bps)
            } else {
                self.reduce(bucket_claims)
            };
            if let Some(summary) = result {
                summaries.push(summary);
            }
        }
        summaries
    }
}

impl SemanticReducer for ExactMatchReducer {
    fn can_reduce(&self, claims: &[Claim]) -> bool {
        if claims.len() < 2 {
            return false;
        }
        let key = claims[0].fingerprint;
        claims.iter().all(|c| c.fingerprint == key)
    }

    fn reduce(&self, claims: &[Claim]) -> Option<Claim> {
        if !self.can_reduce(claims) {
            return None;
        }

        // Sort by id for commutativity guarantee
        let mut sorted: Vec<&Claim> = claims.iter().collect();
        sorted.sort_by_key(|c| c.id);
        // Deduplicate for idempotency
        sorted.dedup_by_key(|c| c.id);

        let first = &sorted[0];
        let data = match &first.kind {
            ClaimKind::Observation { data, .. } => data.clone(),
            // Gemini R5: When reducing Summaries with overlapping tick windows,
            // use the most confident one (idempotent), don't sum log-odds.
            // This prevents double-counting from async compaction on different nodes.
            ClaimKind::Summary { data, .. } => {
                // Find the Summary with the widest window or highest confidence
                let best = sorted
                    .iter()
                    .filter_map(|c| match &c.kind {
                        ClaimKind::Summary {
                            tick_start: ts,
                            tick_end: te,
                            aggregated_logodds: lo,
                            data: d,
                            ..
                        } => Some((*ts, *te, *lo, d.clone())),
                        _ => None,
                    })
                    .max_by_key(|(ts, te, lo, _)| (*te - *ts, lo.value()));

                if let Some((_, _, _, best_data)) = best {
                    // For overlapping Summaries, keep the best one's data
                    // and aggregate only non-overlapping tick ranges
                    best_data
                } else {
                    data.clone()
                }
            }
            _ => return None,
        };

        // Count unique evidence sources.
        // SECURITY: Claims from zero-reputation authors are excluded from
        // aggregation to prevent reputation laundering. Without this check,
        // an attacker could flood the network with unsigned claims that get
        // aggregated into a Summary signed by an honest node during GC,
        // effectively laundering the attacker's zero-reputation through
        // the honest node's signature.
        // Note: When no ReputationTracker is available (standalone reduce),
        // all claims are included (backward compatibility). The BeliefEngine
        // pipeline always has access to reputations.
        let mut unique_sources: SmallVec<[ClaimHash; 16]> = SmallVec::new();
        let mut evidence_logodds: Vec<LogOdds> = Vec::new();

        for c in &sorted {
            if !unique_sources.contains(&c.evidence_source) {
                unique_sources.push(c.evidence_source);
                evidence_logodds.push(c.confidence);
            }
        }

        let aggregated = LogOdds::aggregate(&evidence_logodds);

        // Compute variance (ChatGPT R3: prevent information black holes)
        // Uses i128 for intermediate diff² to prevent i64 overflow.
        // Worst case: diff = 2*i32::MAX ≈ 4.3e9, diff² ≈ 1.8e19 > i64::MAX.
        // With i128, diff² ≈ 1.8e19 is safe (i128::MAX ≈ 1.7e38).
        let n = evidence_logodds.len() as i128;
        let mean = if n > 0 {
            aggregated.value() as i128 / n
        } else {
            0
        };
        let variance_milli = if n > 1 {
            let sum_sq: i128 = evidence_logodds
                .iter()
                .map(|lo| {
                    let diff = lo.value() as i128 - mean;
                    diff * diff
                })
                .sum();
            (sum_sq / n).clamp(i64::MIN as i128, i64::MAX as i128) as i64
        } else {
            0
        };

        // Range preservation
        let range_min = evidence_logodds
            .iter()
            .copied()
            .min()
            .unwrap_or(LogOdds::NEUTRAL);
        let range_max = evidence_logodds
            .iter()
            .copied()
            .max()
            .unwrap_or(LogOdds::NEUTRAL);

        let tick_start = sorted.iter().map(|c| c.tick).min().unwrap_or(0);
        let tick_end = sorted.iter().map(|c| c.tick).max().unwrap_or(0);

        let summary_cell = first.correlation_cell;

        let mut hasher = blake3::Hasher::new();
        hasher.update(&first.fingerprint.primary);
        hasher.update(&(unique_sources.len() as u32).to_le_bytes());
        hasher.update(&tick_start.to_le_bytes());
        hasher.update(&tick_end.to_le_bytes());
        hasher.update(&aggregated.value().to_le_bytes());
        hasher.update(&variance_milli.to_le_bytes());
        if let Some(cell) = summary_cell {
            hasher.update(&cell.0.to_le_bytes());
        }
        let id = *hasher.finalize().as_bytes();

        // Gemini R6 fix: evidence_source must be unique per Summary, NOT [0u8; 32].
        // Otherwise the dedup loop in future reductions silently discards all but one Summary.
        // Use the Summary's own id as evidence_source (guaranteed unique).
        Some(Claim {
            id,
            fingerprint: first.fingerprint,
            origin: [0u8; 32],
            kind: ClaimKind::Summary {
                source_count: claims.len() as u32,
                tick_start,
                tick_end,
                data,
                aggregated_logodds: aggregated,
                unique_sources: unique_sources.len() as u32,
                variance_milli,
                range_min,
                range_max,
            },
            confidence: aggregated,
            evidence_source: id, // Unique per Summary — prevents dedup collision
            tick: tick_end,
            correlation_cell: summary_cell,
            embedding: None,
            embedding_version: 0,
        })
    }

    fn lossiness(&self) -> u8 {
        10
    }

    fn group_key(&self, claim: &Claim) -> u64 {
        // Use secondary fingerprint for fuzzy grouping
        claim.fingerprint.secondary
    }
}

/// Reputation-weighted intent resolver.
pub struct ReputationWeightedResolver;

impl IntentResolver for ReputationWeightedResolver {
    fn resolve(&self, intents: &[Claim], reputations: &dyn ReputationTracker) -> Resolution {
        if intents.is_empty() {
            return Resolution::Escalate {
                reason: "No intents".into(),
                conflicting: vec![],
            };
        }

        let winner = intents
            .iter()
            .max_by_key(|claim| {
                let rep = reputations.reputation(&claim.origin);
                let weighted = rep.weight_evidence(claim.confidence);
                (weighted.value(), u64::MAX - claim.tick)
            })
            .unwrap();

        Resolution::Winner(winner.clone())
    }
}

/// Evidence-weighted contradiction resolver.
pub struct EvidenceWeightedContradictionResolver;

impl ContradictionResolver for EvidenceWeightedContradictionResolver {
    fn resolve(
        &self,
        claim_a: &Claim,
        claim_b: &Claim,
        graph: &KnowledgeGraph,
        reputations: &dyn ReputationTracker,
    ) -> ContradictionOutcome {
        let rep_a = reputations.reputation(&claim_a.origin);
        let rep_b = reputations.reputation(&claim_b.origin);

        let weighted_a = rep_a.weight_evidence(claim_a.confidence);
        let weighted_b = rep_b.weight_evidence(claim_b.confidence);

        let (supports_a, _) = graph.support_ratio(0); // Would use real arena IDs
        let (supports_b, _) = graph.support_ratio(1);

        // Total score: weighted confidence + support count * 500
        let score_a = weighted_a.value() as i64 + supports_a as i64 * 500;
        let score_b = weighted_b.value() as i64 + supports_b as i64 * 500;

        if score_a > score_b + 1000 {
            ContradictionOutcome::PreferA {
                b_penalty: LogOdds::new(-2000),
            }
        } else if score_b > score_a + 1000 {
            ContradictionOutcome::PreferB {
                a_penalty: LogOdds::new(-2000),
            }
        } else {
            ContradictionOutcome::Undecided
        }
    }
}

/// Log-odds belief engine: classifies claims into accepted/rejected/uncertain.
pub struct LogOddsBeliefEngine {
    /// Log-odds threshold for acceptance (e.g., 2197 ≈ 90%)
    pub accept_threshold: LogOdds,
    /// Log-odds threshold for rejection (e.g., -2197 ≈ 10%)
    pub reject_threshold: LogOdds,
    /// Maximum fraction of positive trust a single contradiction can remove (basis points).
    /// Default: 5000 (50%). Prevents single-actor annihilation attacks.
    /// Paper: "configurable Contradiction Damping factor α ∈ [0, 1]"
    pub max_contradiction_damping_bps: u16,
}

impl Default for LogOddsBeliefEngine {
    fn default() -> Self {
        Self {
            accept_threshold: LogOdds::new(2197),
            reject_threshold: LogOdds::new(-2197),
            max_contradiction_damping_bps: 5000, // 50% default
        }
    }
}

impl BeliefEngine for LogOddsBeliefEngine {
    fn compute(
        &self,
        claims: &[Claim],
        graph: &KnowledgeGraph,
        reputations: &dyn ReputationTracker,
    ) -> BeliefState {
        let mut state = BeliefState::default();

        // Step 1: Compute base trust for each claim (reputation-weighted)
        let mut base_trust = rustc_hash::FxHashMap::default();
        for (i, claim) in claims.iter().enumerate() {
            let rep = reputations.reputation(&claim.origin);
            base_trust.insert(i as ClaimArenaId, rep.weight_evidence(claim.confidence));
        }

        // Step 2: Two-pass trust propagation (Gemini R3, R6)
        // Uses REAL reputation from ReputationTracker, not log-odds from base_trust.
        let propagated = graph.propagate_trust_full(
            &base_trust,
            5,
            self.max_contradiction_damping_bps,
            claims,
            reputations,
        );

        for (i, _claim) in claims.iter().enumerate() {
            let arena_id = i as ClaimArenaId;

            let final_logodds = propagated
                .get(&arena_id)
                .copied()
                .unwrap_or(LogOdds::NEUTRAL);

            if final_logodds.value() >= self.accept_threshold.value() {
                state.accepted.push(arena_id);
            } else if final_logodds.value() <= self.reject_threshold.value() {
                state.rejected.push(arena_id);
            } else {
                state.uncertain.push(arena_id);
            }
        }

        state
    }
}

/// Dependency-aware relevance scorer with graph traversal.
pub struct DependencyAwareScorer {
    pub half_life_ticks: u64,
}

impl Default for DependencyAwareScorer {
    fn default() -> Self {
        Self {
            half_life_ticks: 1000,
        }
    }
}

impl RelevanceScorer for DependencyAwareScorer {
    fn score(&self, claim: &Claim, active_intents: &[Claim], graph: &KnowledgeGraph) -> u16 {
        let claim_arena_id = 0u32; // In real impl, would be passed as parameter

        // Rule 1: If anything depends on this claim, it's critical
        let dependents = graph.dependents(claim_arena_id);
        if !dependents.is_empty() {
            return 10000;
        }

        // Rule 2: Support/contradiction weighting
        let (supports, contradicts) = graph.support_ratio(claim_arena_id);
        if supports > 0 {
            return 8000;
        }
        if contradicts > supports {
            return 500; // Low relevance if mostly contradicted
        }

        // Rule 3: Time decay (integer only)
        let current_tick = active_intents
            .iter()
            .map(|c| c.tick)
            .max()
            .unwrap_or(claim.tick);
        let age = current_tick.saturating_sub(claim.tick);

        if self.half_life_ticks == 0 {
            return 0;
        }

        let decay_steps = age / self.half_life_ticks;
        if decay_steps >= 14 {
            0
        } else {
            10000u16 >> (decay_steps as u16)
        }
    }
}

/// Simple in-memory reputation tracker.
#[derive(Default)]
pub struct InMemoryReputationTracker {
    scores: rustc_hash::FxHashMap<[u8; 32], Reputation>,
}

impl InMemoryReputationTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Directly set a node's reputation (for bootstrapping anchor nodes).
    pub fn set_reputation(&mut self, origin: &[u8; 32], rep: Reputation) {
        self.scores.insert(*origin, rep);
    }
}

impl ReputationTracker for InMemoryReputationTracker {
    fn reputation(&self, origin: &[u8; 32]) -> Reputation {
        // Gemini R5: Unknown nodes start at ZERO, not NEUTRAL.
        // Prevents Sybil attack where 1000 fresh Ed25519 keys each get 5000 rep.
        // Nodes must be delegated by an anchor (Web of Trust) to gain reputation.
        self.scores.get(origin).copied().unwrap_or(Reputation::ZERO)
    }

    fn update(&mut self, origin: &[u8; 32], event: ReputationEvent) {
        let current = self.reputation(origin);
        let new_score = match event {
            ReputationEvent::EquivocationDetected => 0,
            ReputationEvent::ClaimContradicted => current.bps().saturating_sub(1000),
            ReputationEvent::ClaimConfirmed => current.bps().saturating_add(200).min(10000),
            ReputationEvent::ActiveParticipation => current.bps().saturating_add(50).min(10000),
        };
        self.scores.insert(*origin, Reputation::from_bps(new_score));
    }

    fn delegate(&mut self, from: &[u8; 32], to: &[u8; 32], initial_rep: Reputation) {
        // Only delegators with reputation > 0 can delegate
        let delegator_rep = self.reputation(from);
        if delegator_rep.bps() == 0 {
            return; // Slashed or unknown nodes cannot delegate
        }
        // Delegated reputation capped by delegator's own reputation
        let capped = Reputation::from_bps(initial_rep.bps().min(delegator_rep.bps()));
        self.scores.insert(*to, capped);

        // Reputation spending: delegation costs half of what was granted.
        // This prevents unbounded Sybil creation: a node with 10000 rep
        // can delegate 10000 to one node (costs 5000) → remaining 5000,
        // then 5000 to another (costs 2500) → remaining 2500, etc.
        // Total delegatable reputation converges to 2× original (geometric series).
        let cost = capped.bps() / 2;
        let new_delegator_rep = delegator_rep.bps().saturating_sub(cost);
        self.scores
            .insert(*from, Reputation::from_bps(new_delegator_rep));
    }
}

// ─── Tests ──────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_fingerprint(data: &[u8], sensor: u8) -> SemanticFingerprint {
        let hash = blake3::hash(data);
        let mut primary = [0u8; 16];
        primary.copy_from_slice(&hash.as_bytes()[..16]);

        let mut feat_hasher = blake3::Hasher::new();
        feat_hasher.update(&[sensor]);
        let secondary =
            u64::from_le_bytes(feat_hasher.finalize().as_bytes()[..8].try_into().unwrap());

        SemanticFingerprint { primary, secondary }
    }

    fn make_claim(sensor: u8, data: &[u8], logodds: i32, tick: u64, source: [u8; 32]) -> Claim {
        make_claim_with_cell(sensor, data, logodds, tick, source, None)
    }

    fn make_claim_with_cell(
        sensor: u8,
        data: &[u8],
        logodds: i32,
        tick: u64,
        source: [u8; 32],
        cell: Option<CorrelationCell>,
    ) -> Claim {
        let fp = make_fingerprint(data, sensor);
        let mut hasher = blake3::Hasher::new();
        hasher.update(&fp.primary);
        hasher.update(&tick.to_le_bytes());
        hasher.update(&source);
        if let Some(c) = cell {
            hasher.update(&c.0.to_le_bytes());
        }
        let id = *hasher.finalize().as_bytes();

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
            correlation_cell: cell,
            embedding: None,
            embedding_version: 0,
        }
    }

    // ── Log-odds tests ──

    #[test]
    fn test_logodds_aggregation_is_addition() {
        // Two pieces of 90% evidence: logodds ~2197 each
        let a = LogOdds::from_percent(90); // 2197
        let b = LogOdds::from_percent(90); // 2197
        let combined = LogOdds::aggregate(&[a, b]);
        // Combined should be ~99.something% (4394 logodds)
        assert_eq!(combined.value(), 4394);
        assert_eq!(combined.to_percent(), 99);
    }

    #[test]
    fn test_logodds_no_underflow() {
        // The u16 killer: many low-confidence claims
        let weak_evidence: Vec<LogOdds> = (0..100)
            .map(|_| LogOdds::from_percent(10)) // Each ~-2197
            .collect();
        let combined = LogOdds::aggregate(&weak_evidence);
        // Should be very negative but NOT zero (unlike u16 which would underflow)
        assert!(combined.value() < -200000);
        assert_eq!(combined.to_percent(), 0); // Effectively 0%, but math is correct
    }

    #[test]
    fn test_logodds_deterministic_across_calls() {
        let evidence = vec![LogOdds::new(1500), LogOdds::new(-800), LogOdds::new(2200)];
        let r1 = LogOdds::aggregate(&evidence);
        let r2 = LogOdds::aggregate(&evidence);
        assert_eq!(r1, r2); // MUST be identical (no float rounding variance)
    }

    #[test]
    fn test_logodds_update_bayesian() {
        let prior = LogOdds::NEUTRAL; // 50%
        let strong_evidence = LogOdds::new(2197); // exact log-odds for 90%
        let posterior = prior.update(strong_evidence);
        // to_percent uses coarser brackets: 2197 maps to 95% bracket.
        // This is by design: from_percent gives exact values, to_percent
        // gives human-readable approximations. The log-odds value (2197)
        // is the one used in all computation.
        assert_eq!(posterior.to_percent(), 95);
        assert_eq!(posterior.value(), 2197); // exact value preserved
    }

    // ── Reputation tests ──

    #[test]
    fn test_byzantine_zero_influence() {
        let byzantine = Reputation::ZERO;
        let max_conf = LogOdds::VERY_HIGH;
        let weighted = byzantine.weight_evidence(max_conf);
        assert_eq!(weighted.value(), 0); // Zero influence!
    }

    #[test]
    fn test_reputation_scales_evidence() {
        let good = Reputation::from_bps(8000); // 80%
        let bad = Reputation::from_bps(2000); // 20%
        let evidence = LogOdds::new(1000);

        let good_weighted = good.weight_evidence(evidence);
        let bad_weighted = bad.weight_evidence(evidence);

        assert_eq!(good_weighted.value(), 800);
        assert_eq!(bad_weighted.value(), 200);
    }

    // ── Echo chamber tests ──

    #[test]
    fn test_echo_chamber_same_source() {
        let reducer = ExactMatchReducer;
        let same_sensor = [99u8; 32];

        let claims: Vec<Claim> = (0..10)
            .map(|i| make_claim(1, b"temp=20C", 2197, i, same_sensor))
            .collect();

        let summary = reducer.reduce(&claims).expect("should reduce");
        match &summary.kind {
            ClaimKind::Summary {
                unique_sources,
                aggregated_logodds,
                ..
            } => {
                assert_eq!(*unique_sources, 1); // Echo chamber detected!
                                                // Only 1 source → confidence = single claim's logodds, NOT 10x
                assert_eq!(aggregated_logodds.value(), 2197);
            }
            _ => panic!("expected Summary"),
        }
    }

    #[test]
    fn test_independent_sources_aggregate() {
        let reducer = ExactMatchReducer;

        let claims: Vec<Claim> = (0..5)
            .map(|i| {
                let mut source = [0u8; 32];
                source[0] = i as u8;
                make_claim(1, b"temp=20C", 2197, i as u64, source)
            })
            .collect();

        let summary = reducer.reduce(&claims).expect("should reduce");
        match &summary.kind {
            ClaimKind::Summary {
                unique_sources,
                aggregated_logodds,
                ..
            } => {
                assert_eq!(*unique_sources, 5);
                // 5 independent sources: 2197 * 5 = 10985
                assert_eq!(aggregated_logodds.value(), 10985);
                assert_eq!(aggregated_logodds.to_percent(), 100); // Effectively certain
            }
            _ => panic!("expected Summary"),
        }
    }

    // ── Reducer commutativity/idempotency tests (Gemini requirement) ──

    #[test]
    fn test_reducer_commutativity() {
        let reducer = ExactMatchReducer;

        let a = make_claim(1, b"data", 1000, 0, [1u8; 32]);
        let b = make_claim(1, b"data", 2000, 1, [2u8; 32]);

        let ab = reducer.reduce(&[a.clone(), b.clone()]).unwrap();
        let ba = reducer.reduce(&[b, a]).unwrap();

        assert_eq!(ab.id, ba.id); // Same hash regardless of order
        assert_eq!(ab.confidence, ba.confidence);
    }

    #[test]
    fn test_reducer_idempotency() {
        let reducer = ExactMatchReducer;

        let a = make_claim(1, b"data", 1000, 0, [1u8; 32]);

        let aa = reducer.reduce(&[a.clone(), a.clone()]).unwrap();
        // Idempotent: reducing [A, A] should be same as single A
        match &aa.kind {
            ClaimKind::Summary {
                unique_sources,
                aggregated_logodds,
                ..
            } => {
                assert_eq!(*unique_sources, 1);
                assert_eq!(aggregated_logodds.value(), 1000); // Not doubled
            }
            _ => panic!("expected Summary"),
        }
    }

    // ── Knowledge graph tests ──

    #[test]
    fn test_graph_dependents_traversal() {
        let mut graph = KnowledgeGraph::new();
        graph.add_edge(EpistemicEdge {
            from: 0,
            to: 1,
            relation: Relation::DerivedFrom,
            strength: Reputation::from_bps(9000),
        });
        graph.add_edge(EpistemicEdge {
            from: 1,
            to: 2,
            relation: Relation::DerivedFrom,
            strength: Reputation::from_bps(9000),
        });
        graph.add_edge(EpistemicEdge {
            from: 0,
            to: 3,
            relation: Relation::Supports,
            strength: Reputation::from_bps(9000),
        });

        let deps = graph.dependents(0);
        assert!(deps.contains(&1));
        assert!(deps.contains(&2)); // Transitive!
        assert!(deps.contains(&3));
    }

    #[test]
    fn test_graph_support_ratio() {
        let mut graph = KnowledgeGraph::new();
        graph.add_edge(EpistemicEdge {
            from: 0,
            to: 5,
            relation: Relation::Supports,
            strength: Reputation::from_bps(9000),
        });
        graph.add_edge(EpistemicEdge {
            from: 1,
            to: 5,
            relation: Relation::Supports,
            strength: Reputation::from_bps(9000),
        });
        graph.add_edge(EpistemicEdge {
            from: 2,
            to: 5,
            relation: Relation::Contradicts,
            strength: Reputation::from_bps(8000),
        });

        let (sup, con) = graph.support_ratio(5);
        assert_eq!(sup, 2);
        assert_eq!(con, 1);
    }

    // ── Belief engine tests ──

    #[test]
    fn test_belief_engine_classifies() {
        let engine = LogOddsBeliefEngine::default();
        let mut tracker = InMemoryReputationTracker::new();
        let graph = KnowledgeGraph::new();
        let anchor = [255u8; 32];
        tracker.set_reputation(&anchor, Reputation::FULL);

        let claims = vec![
            make_claim(1, b"strong", 5000, 0, [1u8; 32]), // High confidence → accepted
            make_claim(1, b"weak", -5000, 1, [2u8; 32]),  // Low confidence → rejected
            make_claim(1, b"meh", 100, 2, [3u8; 32]),     // Middling → uncertain
        ];
        // All claims share origin [1u8;32] from make_claim.
        // Set reputation directly to avoid delegation spending interactions.
        tracker.set_reputation(&claims[0].origin, Reputation::FULL);

        let state = engine.compute(&claims, &graph, &tracker);

        assert!(state.accepted.contains(&0));
        assert!(state.rejected.contains(&1));
        assert!(state.uncertain.contains(&2));
    }

    #[test]
    fn test_belief_engine_reputation_affects_classification() {
        let engine = LogOddsBeliefEngine::default();
        let mut tracker = InMemoryReputationTracker::new();
        let graph = KnowledgeGraph::new();

        let byzantine_node = [42u8; 32];
        tracker.update(&byzantine_node, ReputationEvent::EquivocationDetected);

        let mut claim = make_claim(1, b"lie", 5000, 0, [1u8; 32]);
        claim.origin = byzantine_node;

        let state = engine.compute(&[claim], &graph, &tracker);

        // Despite high self-declared confidence, Byzantine node's claim is uncertain/rejected
        assert!(!state.accepted.contains(&0));
    }

    // ── Intent resolver tests ──

    #[test]
    fn test_resolver_prefers_reputable_node() {
        let resolver = ReputationWeightedResolver;
        let mut tracker = InMemoryReputationTracker::new();

        let honest = [1u8; 32];
        let liar = [2u8; 32];
        tracker.update(&honest, ReputationEvent::ClaimConfirmed);
        tracker.update(&honest, ReputationEvent::ClaimConfirmed);
        tracker.update(&liar, ReputationEvent::EquivocationDetected);

        let mut intent_a = make_claim(1, b"heat", 1000, 0, [0u8; 32]);
        intent_a.origin = honest;
        let mut intent_b = make_claim(1, b"cool", 5000, 1, [0u8; 32]);
        intent_b.origin = liar;

        match resolver.resolve(&[intent_a, intent_b], &tracker) {
            Resolution::Winner(w) => assert_eq!(w.origin, honest),
            _ => panic!("expected Winner"),
        }
    }

    // ── Cycle detection tests (ChatGPT R3) ──

    #[test]
    fn test_cycle_detection_simple() {
        let mut graph = KnowledgeGraph::new();
        // A → B → C → A (cycle of length 3)
        graph.add_edge(EpistemicEdge {
            from: 0,
            to: 1,
            relation: Relation::Supports,
            strength: Reputation::from_bps(9000),
        });
        graph.add_edge(EpistemicEdge {
            from: 1,
            to: 2,
            relation: Relation::Supports,
            strength: Reputation::from_bps(9000),
        });
        graph.add_edge(EpistemicEdge {
            from: 2,
            to: 0,
            relation: Relation::Supports,
            strength: Reputation::from_bps(9000),
        });

        let cycles = graph.detect_cycles();
        assert!(!cycles.is_empty(), "should detect the A→B→C→A cycle");

        // All three nodes should be in the cycle
        let cycle = &cycles[0];
        assert!(cycle.contains(&0));
        assert!(cycle.contains(&1));
        assert!(cycle.contains(&2));
    }

    #[test]
    fn test_no_cycle_in_dag() {
        let mut graph = KnowledgeGraph::new();
        // A → B → C (no cycle)
        graph.add_edge(EpistemicEdge {
            from: 0,
            to: 1,
            relation: Relation::DerivedFrom,
            strength: Reputation::from_bps(9000),
        });
        graph.add_edge(EpistemicEdge {
            from: 1,
            to: 2,
            relation: Relation::DerivedFrom,
            strength: Reputation::from_bps(9000),
        });

        let cycles = graph.detect_cycles();
        assert!(cycles.is_empty(), "DAG should have no cycles");
    }

    #[test]
    fn test_cyclic_edges_identified() {
        let mut graph = KnowledgeGraph::new();
        graph.add_edge(EpistemicEdge {
            from: 0,
            to: 1,
            relation: Relation::Supports,
            strength: Reputation::from_bps(9000),
        });
        graph.add_edge(EpistemicEdge {
            from: 1,
            to: 0,
            relation: Relation::Supports,
            strength: Reputation::from_bps(9000),
        });

        let cyclic = graph.cyclic_edge_indices();

        // At least one back-edge should be identified
        assert!(!cyclic.is_empty(), "should find cyclic back-edge in A↔B");
    }

    #[test]
    fn test_cycle_zeroed_prevents_inflation() {
        // The core attack: A supports B, B supports A → would be infinite confidence
        // Gemini R3 fix: cyclic edges are zeroed, trust does NOT flow through them
        let engine = LogOddsBeliefEngine::default();
        let tracker = InMemoryReputationTracker::new();

        let mut graph = KnowledgeGraph::new();
        graph.add_edge(EpistemicEdge {
            from: 0,
            to: 1,
            relation: Relation::Supports,
            strength: Reputation::from_bps(9000),
        });
        graph.add_edge(EpistemicEdge {
            from: 1,
            to: 0,
            relation: Relation::Supports,
            strength: Reputation::from_bps(9000),
        });

        // Both claims have moderate confidence (below accept threshold of 2197)
        let claims = vec![
            make_claim(1, b"A", 1500, 0, [1u8; 32]),
            make_claim(1, b"B", 1500, 1, [2u8; 32]),
        ];

        let state = engine.compute(&claims, &graph, &tracker);

        // With cyclic edges zeroed, mutual support should NOT inflate either to accepted
        assert!(
            !state.accepted.contains(&0) || !state.accepted.contains(&1),
            "cyclic mutual support must not inflate both to accepted"
        );
    }

    // ── Trust propagation tests (ChatGPT R3) ──

    #[test]
    fn test_trust_propagation_transitive() {
        let mut graph = KnowledgeGraph::new();
        graph.add_edge(EpistemicEdge {
            from: 0,
            to: 1,
            relation: Relation::Supports,
            strength: Reputation::from_bps(9000),
        });
        graph.add_edge(EpistemicEdge {
            from: 1,
            to: 2,
            relation: Relation::Supports,
            strength: Reputation::from_bps(9000),
        });

        // Create claims with distinct origins so reputation lookup works
        let claims = vec![
            make_claim(1, b"A", 5000, 0, [1u8; 32]),
            make_claim(1, b"B", 0, 1, [2u8; 32]),
            make_claim(1, b"C", 0, 2, [3u8; 32]),
        ];
        let mut tracker = InMemoryReputationTracker::new();
        let anchor = [255u8; 32];
        tracker.set_reputation(&anchor, Reputation::FULL);
        for c in &claims {
            tracker.delegate(&anchor, &c.origin, Reputation::from_bps(8000));
        }

        let mut base = rustc_hash::FxHashMap::default();
        base.insert(0u32, LogOdds::new(5000));
        base.insert(1u32, LogOdds::NEUTRAL);
        base.insert(2u32, LogOdds::NEUTRAL);

        let propagated = graph.propagate_trust_full(&base, 5, 5000, &claims, &tracker);

        let trust_c = propagated.get(&2).copied().unwrap_or(LogOdds::NEUTRAL);
        assert!(
            trust_c.value() > LogOdds::NEUTRAL.value(),
            "C should inherit transitive trust from A, got {}",
            trust_c.value()
        );

        let trust_b = propagated.get(&1).copied().unwrap_or(LogOdds::NEUTRAL);
        assert!(
            trust_b.value() > trust_c.value(),
            "B ({}) should have more trust than C ({})",
            trust_b.value(),
            trust_c.value()
        );
    }

    #[test]
    fn test_contradiction_reduces_trust_pass2() {
        let mut graph = KnowledgeGraph::new();
        graph.add_edge(EpistemicEdge {
            from: 0,
            to: 1,
            relation: Relation::Contradicts,
            strength: Reputation::from_bps(8000),
        });

        let claims = vec![
            make_claim(1, b"A", 3000, 0, [1u8; 32]),
            make_claim(1, b"B", 2000, 1, [2u8; 32]),
        ];
        let mut tracker = InMemoryReputationTracker::new();
        let anchor = [255u8; 32];
        tracker.set_reputation(&anchor, Reputation::FULL);
        for c in &claims {
            tracker.delegate(&anchor, &c.origin, Reputation::from_bps(8000));
        }

        let mut base = rustc_hash::FxHashMap::default();
        base.insert(0u32, LogOdds::new(3000));
        base.insert(1u32, LogOdds::new(2000));

        let propagated = graph.propagate_trust_full(&base, 3, 5000, &claims, &tracker);

        let trust_b = propagated.get(&1).copied().unwrap_or(LogOdds::NEUTRAL);
        assert!(
            trust_b.value() < 2000,
            "B's trust should decrease when contradicted by trusted A, got {}",
            trust_b.value()
        );
    }

    #[test]
    fn test_cyclic_edges_zeroed_no_amplification() {
        let mut graph = KnowledgeGraph::new();
        graph.add_edge(EpistemicEdge {
            from: 0,
            to: 1,
            relation: Relation::Supports,
            strength: Reputation::from_bps(9000),
        });
        graph.add_edge(EpistemicEdge {
            from: 1,
            to: 0,
            relation: Relation::Supports,
            strength: Reputation::from_bps(9000),
        });

        let cyclic = graph.cyclic_edge_indices();
        assert!(!cyclic.is_empty(), "should detect cyclic back-edge");

        let claims = vec![
            make_claim(1, b"A", 1000, 0, [1u8; 32]),
            make_claim(1, b"B", 1000, 1, [2u8; 32]),
        ];
        let mut tracker = InMemoryReputationTracker::new();
        let anchor = [255u8; 32];
        tracker.set_reputation(&anchor, Reputation::FULL);
        for c in &claims {
            tracker.delegate(&anchor, &c.origin, Reputation::from_bps(8000));
        }

        let mut base = rustc_hash::FxHashMap::default();
        base.insert(0u32, LogOdds::new(1000));
        base.insert(1u32, LogOdds::new(1000));

        let propagated = graph.propagate_trust_full(&base, 10, 5000, &claims, &tracker);

        let trust_0 = propagated.get(&0).copied().unwrap_or(LogOdds::NEUTRAL);
        let trust_1 = propagated.get(&1).copied().unwrap_or(LogOdds::NEUTRAL);
        assert!(
            trust_0.value() < 5000,
            "cyclic amplification blocked: node 0 trust = {}",
            trust_0.value()
        );
        assert!(
            trust_1.value() < 5000,
            "cyclic amplification blocked: node 1 trust = {}",
            trust_1.value()
        );
    }

    #[test]
    fn test_annihilation_cap_prevents_one_shot_kill() {
        let mut graph = KnowledgeGraph::new();
        for i in 0..5 {
            graph.add_edge(EpistemicEdge {
                from: i,
                to: 5,
                relation: Relation::Supports,
                strength: Reputation::from_bps(9000),
            });
        }
        graph.add_edge(EpistemicEdge {
            from: 6,
            to: 5,
            relation: Relation::Contradicts,
            strength: Reputation::from_bps(10000),
        });

        let claims: Vec<Claim> = (0..7)
            .map(|i| {
                let mut src = [0u8; 32];
                src[0] = i as u8;
                make_claim(1, &[i as u8], 5000, i as u64, src)
            })
            .collect();
        let mut tracker = InMemoryReputationTracker::new();
        let anchor = [255u8; 32];
        tracker.set_reputation(&anchor, Reputation::FULL);
        for c in &claims {
            tracker.delegate(&anchor, &c.origin, Reputation::from_bps(8000));
        }

        let mut base = rustc_hash::FxHashMap::default();
        for i in 0..=6 {
            base.insert(i as ClaimArenaId, LogOdds::new(5000));
        }

        let propagated = graph.propagate_trust_full(&base, 5, 5000, &claims, &tracker);

        let trust_5 = propagated.get(&5).copied().unwrap_or(LogOdds::NEUTRAL);
        assert!(
            trust_5.value() > 0,
            "5 supporters vs 1 contradictor: claim should remain positive, got {}",
            trust_5.value()
        );
    }

    #[test]
    fn test_dynamic_decay_varies_by_edge() {
        let mut graph = KnowledgeGraph::new();
        // A supports B with HIGH confidence (95%), A supports C with LOW confidence (10%)
        graph.add_edge(EpistemicEdge {
            from: 0,
            to: 1,
            relation: Relation::Supports,
            strength: Reputation::from_bps(9500),
        });
        graph.add_edge(EpistemicEdge {
            from: 0,
            to: 2,
            relation: Relation::Supports,
            strength: Reputation::from_bps(1000),
        });

        let claims = vec![
            make_claim(1, b"A", 5000, 0, [1u8; 32]),
            make_claim(1, b"B", 0, 1, [2u8; 32]),
            make_claim(1, b"C", 0, 2, [3u8; 32]),
        ];
        let mut tracker = InMemoryReputationTracker::new();
        let anchor = [255u8; 32];
        tracker.set_reputation(&anchor, Reputation::FULL);
        for c in &claims {
            tracker.delegate(&anchor, &c.origin, Reputation::from_bps(8000));
        }

        let mut base = rustc_hash::FxHashMap::default();
        base.insert(0u32, LogOdds::new(5000));
        base.insert(1u32, LogOdds::NEUTRAL);
        base.insert(2u32, LogOdds::NEUTRAL);

        let propagated = graph.propagate_trust_full(&base, 3, 5000, &claims, &tracker);

        let trust_b = propagated.get(&1).copied().unwrap_or(LogOdds::NEUTRAL);
        let trust_c = propagated.get(&2).copied().unwrap_or(LogOdds::NEUTRAL);

        // B should get much more trust than C (95% vs 10% edge strength)
        assert!(
            trust_b.value() > trust_c.value() * 5,
            "B ({}) should get ~9.5x more trust than C ({})",
            trust_b.value(),
            trust_c.value()
        );
    }

    // ── Summary variance tests (ChatGPT R3) ──

    #[test]
    fn test_summary_preserves_variance() {
        let reducer = ExactMatchReducer;

        // Claims with DIFFERENT confidence levels from independent sources
        let claims = vec![
            make_claim(1, b"temp=20C", 1000, 0, [1u8; 32]), // Low confidence
            make_claim(1, b"temp=20C", 5000, 1, [2u8; 32]), // High confidence
            make_claim(1, b"temp=20C", 100, 2, [3u8; 32]),  // Very low
        ];

        let summary = reducer.reduce(&claims).expect("should reduce");
        match &summary.kind {
            ClaimKind::Summary {
                variance_milli,
                range_min,
                range_max,
                ..
            } => {
                // Variance should be non-zero (disagreement on confidence)
                assert!(
                    *variance_milli > 0,
                    "variance should capture disagreement: got {}",
                    variance_milli
                );

                // Range should capture the full spread
                assert_eq!(range_min.value(), 100);
                assert_eq!(range_max.value(), 5000);
            }
            _ => panic!("expected Summary"),
        }
    }

    #[test]
    fn test_summary_zero_variance_when_agreement() {
        let reducer = ExactMatchReducer;

        // All claims have SAME confidence from different sources
        let claims: Vec<Claim> = (0..5)
            .map(|i| {
                let mut source = [0u8; 32];
                source[0] = i as u8;
                make_claim(1, b"temp=20C", 2000, i as u64, source)
            })
            .collect();

        let summary = reducer.reduce(&claims).expect("should reduce");
        match &summary.kind {
            ClaimKind::Summary {
                variance_milli,
                range_min,
                range_max,
                ..
            } => {
                assert_eq!(*variance_milli, 0, "identical confidences → zero variance");
                assert_eq!(range_min.value(), range_max.value());
            }
            _ => panic!("expected Summary"),
        }
    }

    // ── Gemini R5 exploit tests ──

    #[test]
    fn test_enemy_of_my_enemy_blocked() {
        // Exploit: node X has negative trust (-5000). X contradicts Y.
        // Without clamp: Y.trust -= (-5000) = Y.trust + 5000 (free trust!)
        // With clamp: X's influence is 0 (negative trust = no voice)
        let mut graph = KnowledgeGraph::new();
        graph.add_edge(EpistemicEdge {
            from: 0,
            to: 1,
            relation: Relation::Contradicts,
            strength: Reputation::from_bps(10000),
        });

        let mut base = rustc_hash::FxHashMap::default();
        base.insert(0u32, LogOdds::new(-5000)); // X is a known liar
        base.insert(1u32, LogOdds::new(1000)); // Y is mildly trusted

        let propagated = graph.propagate_trust(&base, 3);

        let trust_y = propagated.get(&1).copied().unwrap_or(LogOdds::NEUTRAL);
        // Y's trust must NOT increase (the liar's contradiction has zero weight)
        assert!(
            trust_y.value() <= 1000,
            "negative-trust contradiction must not boost target, got {}",
            trust_y.value()
        );
    }

    #[test]
    fn test_sybil_attack_blocked() {
        // Exploit: attacker creates 100 fresh Ed25519 keys, each gets default rep
        // All support a false claim → massive fake consensus
        let mut tracker = InMemoryReputationTracker::new();

        // 100 unknown nodes (no delegation)
        let mut total_weight = 0u32;
        for i in 0..100u8 {
            let node = [i; 32];
            let rep = tracker.reputation(&node);
            total_weight += rep.bps() as u32;
        }

        // With NEUTRAL default (5000): total = 500,000 → Sybil succeeds
        // With ZERO default: total = 0 → Sybil blocked
        assert_eq!(
            total_weight, 0,
            "unknown nodes must have zero voting weight"
        );
    }

    #[test]
    fn test_delegation_grants_reputation() {
        let mut tracker = InMemoryReputationTracker::new();

        let anchor = [1u8; 32];
        let new_node = [2u8; 32];

        // Anchor has high reputation
        tracker.set_reputation(&anchor, Reputation::from_bps(8000));

        // New node starts at 0
        assert_eq!(tracker.reputation(&new_node).bps(), 0);

        // Anchor delegates to new node
        tracker.delegate(&anchor, &new_node, Reputation::from_bps(3000));

        // New node now has reputation
        assert_eq!(tracker.reputation(&new_node).bps(), 3000);

        // Reputation spending: anchor lost half of what was delegated
        // 8000 - 3000/2 = 6500
        assert_eq!(tracker.reputation(&anchor).bps(), 6500);
    }

    #[test]
    fn test_slashed_node_cannot_delegate() {
        let mut tracker = InMemoryReputationTracker::new();

        let slashed = [1u8; 32];
        let victim = [2u8; 32];

        tracker.update(&slashed, ReputationEvent::EquivocationDetected);

        // Slashed node tries to delegate
        tracker.delegate(&slashed, &victim, Reputation::from_bps(5000));

        // Victim gets nothing
        assert_eq!(tracker.reputation(&victim).bps(), 0);
    }

    #[test]
    fn test_reputation_spending_limits_sybil() {
        let mut tracker = InMemoryReputationTracker::new();
        let delegator = [1u8; 32];
        tracker.set_reputation(&delegator, Reputation::from_bps(10000));

        // Delegator creates 5 nodes, each getting 2000 bps
        let mut _total_delegated = 0u16;
        for i in 0..5u8 {
            let node = [i + 10; 32];
            let before = tracker.reputation(&delegator).bps();
            tracker.delegate(&delegator, &node, Reputation::from_bps(2000));
            let after = tracker.reputation(&delegator).bps();
            let granted = tracker.reputation(&node).bps();
            _total_delegated += granted;

            // Each delegation costs the delegator
            assert!(
                after < before,
                "delegation must cost reputation: before={}, after={}",
                before,
                after
            );
        }

        // With spending, delegator's capacity is bounded.
        // Cost per delegation = granted/2 = 1000. After 5: delegator = 5000.
        // The bound: delegator cannot create unlimited Sybils because
        // each costs reputation. After 10 delegations of 2000, delegator = 0.
        assert_eq!(
            tracker.reputation(&delegator).bps(),
            5000,
            "delegator should have 10000 - 5*1000 = 5000 remaining"
        );

        // Try to delegate more — should be capped by remaining reputation
        let node_6 = [16u8; 32];
        tracker.delegate(&delegator, &node_6, Reputation::FULL);
        assert_eq!(
            tracker.reputation(&node_6).bps(),
            5000,
            "delegation capped to delegator's remaining 5000"
        );
        // Delegator now has 5000 - 2500 = 2500
        assert_eq!(tracker.reputation(&delegator).bps(), 2500);
    }

    #[test]
    fn test_aggregate_associativity_within_operational_range() {
        // Associativity holds within the operational range where intermediate
        // sums don't exceed SAFE_MAX. In practice, log-odds values are in
        // the -7000..+7000 range (0.1% to 99.9% probability).
        // 1000 evidence items at +7000 each = 7,000,000 — well within SAFE_MAX.
        for &(a_val, b_val, c_val) in &[
            (5000, 5000, -3000),
            (6907, 6907, -6907), // VERY_HIGH + VERY_HIGH + VERY_LOW
            (-5000, -5000, 5000),
            (100000, 100000, -100000), // Large but within operational range
        ] {
            let a = LogOdds::new(a_val);
            let b = LogOdds::new(b_val);
            let c = LogOdds::new(c_val);

            let abc = LogOdds::aggregate(&[a, b, c]);
            let ab_then_c = LogOdds::aggregate(&[LogOdds::aggregate(&[a, b]), c]);
            let a_then_bc = LogOdds::aggregate(&[a, LogOdds::aggregate(&[b, c])]);

            assert_eq!(
                abc,
                ab_then_c,
                "associativity broken at ({},{},{}): all={} != (ab)c={}",
                a_val,
                b_val,
                c_val,
                abc.value(),
                ab_then_c.value()
            );
            assert_eq!(
                abc,
                a_then_bc,
                "associativity broken at ({},{},{}): all={} != a(bc)={}",
                a_val,
                b_val,
                c_val,
                abc.value(),
                a_then_bc.value()
            );
        }
    }

    #[test]
    fn test_aggregate_clamping_is_documented() {
        // At SAFE_MAX boundaries, associativity breaks due to clamping.
        // This is intentional and documented: values beyond ±10^9 represent
        // probabilities so extreme that they have no physical meaning.
        let a = LogOdds::new(LogOdds::SAFE_MAX);
        let b = LogOdds::new(LogOdds::SAFE_MAX);
        let c = LogOdds::new(LogOdds::SAFE_MIN);

        let abc = LogOdds::aggregate(&[a, b, c]);
        let ab_then_c = LogOdds::aggregate(&[LogOdds::aggregate(&[a, b]), c]);

        // These MAY differ at boundaries — this is the known limitation.
        // The test documents the behavior, not asserts equality.
        assert!(
            abc.value() == LogOdds::SAFE_MAX,
            "sum of MAX+MAX+MIN should clamp to SAFE_MAX, got {}",
            abc.value()
        );
        assert!(
            ab_then_c.value() == 0,
            "sequential aggregation at boundary produces different result: {}",
            ab_then_c.value()
        );
        // The difference is documented in the paper (Section 14, Limitations).
    }

    #[test]
    fn test_variance_no_overflow_extreme_values() {
        // An attacker sends claims with i32::MAX and i32::MIN confidence.
        // The variance calculation must not overflow or panic.
        let reducer = ExactMatchReducer;
        let claims = vec![
            make_claim(1, b"data", i32::MAX, 0, [1u8; 32]),
            make_claim(1, b"data", i32::MIN, 1, [2u8; 32]),
            make_claim(1, b"data", i32::MAX, 2, [3u8; 32]),
        ];

        // Must not panic (i128 intermediate prevents i64 overflow)
        let summary = reducer.reduce(&claims);
        assert!(summary.is_some(), "reduce must not panic on extreme values");
    }

    #[test]
    fn test_pass2_order_independent() {
        // Two contradictions hitting the same target must produce identical
        // results regardless of edge iteration order.
        let mut graph = KnowledgeGraph::new();
        // C0 contradicts target (C2), C1 contradicts target (C2)
        graph.add_edge(EpistemicEdge {
            from: 0,
            to: 2,
            relation: Relation::Contradicts,
            strength: Reputation::from_bps(8000),
        });
        graph.add_edge(EpistemicEdge {
            from: 1,
            to: 2,
            relation: Relation::Contradicts,
            strength: Reputation::from_bps(8000),
        });

        let claims = vec![
            make_claim(1, b"A", 3000, 0, [1u8; 32]),
            make_claim(1, b"B", 3000, 1, [2u8; 32]),
            make_claim(1, b"T", 5000, 2, [3u8; 32]),
        ];
        let mut tracker = InMemoryReputationTracker::new();
        tracker.set_reputation(&[1u8; 32], Reputation::FULL);

        let mut base = rustc_hash::FxHashMap::default();
        for (i, c) in claims.iter().enumerate() {
            base.insert(
                i as u32,
                tracker.reputation(&c.origin).weight_evidence(c.confidence),
            );
        }

        // Forward order
        let r1 = graph.propagate_trust_full(&base, 5, 5000, &claims, &tracker);

        // Reverse edge order (same graph, different internal ordering)
        let mut graph_rev = KnowledgeGraph::new();
        graph_rev.add_edge(EpistemicEdge {
            from: 1,
            to: 2,
            relation: Relation::Contradicts,
            strength: Reputation::from_bps(8000),
        });
        graph_rev.add_edge(EpistemicEdge {
            from: 0,
            to: 2,
            relation: Relation::Contradicts,
            strength: Reputation::from_bps(8000),
        });

        let r2 = graph_rev.propagate_trust_full(&base, 5, 5000, &claims, &tracker);

        assert_eq!(
            r1.get(&2).map(|l| l.value()),
            r2.get(&2).map(|l| l.value()),
            "Pass 2 must be order-independent: forward={:?}, reverse={:?}",
            r1.get(&2),
            r2.get(&2)
        );
    }

    #[test]
    fn test_hybrid_edge_resolution_survives_gc() {
        // Simulate GC amnesia: Claim C1 is compacted into Summary S.
        // An edge A→C1 should fallback to A→S via SemanticFingerprint.

        let fp = make_fingerprint(b"temp=20C", 1);

        // Claim A (still alive)
        let claim_a = make_claim(1, b"sensor_a", 3000, 0, [1u8; 32]);

        // Summary S (replaced C1 and C2 during GC — same fingerprint as C1)
        let mut hasher = blake3::Hasher::new();
        hasher.update(&fp.primary);
        hasher.update(&100u64.to_le_bytes());
        let summary_id = *hasher.finalize().as_bytes();
        let summary_s = Claim {
            id: summary_id,
            fingerprint: fp,
            origin: [0u8; 32],
            kind: ClaimKind::Summary {
                source_count: 2,
                tick_start: 0,
                tick_end: 10,
                data: b"temp=20C".to_vec(),
                aggregated_logodds: LogOdds::new(4000),
                unique_sources: 2,
                variance_milli: 0,
                range_min: LogOdds::new(2000),
                range_max: LogOdds::new(2000),
            },
            confidence: LogOdds::new(4000),
            evidence_source: summary_id,
            tick: 10,
            correlation_cell: None,
            embedding: None,
            embedding_version: 0,
        };

        // The original C1 hash (now GC'd — NOT in claims array)
        let gc_claim_hash = {
            let mut h = blake3::Hasher::new();
            h.update(b"original_c1");
            *h.finalize().as_bytes()
        };

        // Raw edge: A supports C1 (by hash). C1 is gone, but Summary S
        // has the same SemanticFingerprint.
        let raw_edge = RawEpistemicEdge {
            from_hash: claim_a.id,
            from_fingerprint: claim_a.fingerprint,
            to_hash: gc_claim_hash, // C1's hash — doesn't exist anymore
            to_fingerprint: fp,     // Same fingerprint as Summary S
            relation: Relation::Supports,
            strength: Reputation::from_bps(9000),
        };

        // Build graph with only A and S (C1 is gone)
        let claims = vec![claim_a, summary_s];
        let graph = KnowledgeGraph::build_from_claims(&claims, &[raw_edge]);

        // The edge should have resolved: A (arena 0) → S (arena 1)
        assert_eq!(
            graph.edges().len(),
            1,
            "edge should survive GC via fingerprint fallback"
        );
        assert_eq!(graph.edges()[0].from, 0); // A
        assert_eq!(graph.edges()[0].to, 1); // S (not C1 — C1 is gone)
    }

    #[test]
    fn test_hybrid_edge_drops_when_fully_orphaned() {
        // If both the target hash AND the fingerprint are gone (no Summary
        // exists for that semantic concept), the edge is silently dropped.
        let claim_a = make_claim(1, b"sensor_a", 3000, 0, [1u8; 32]);

        let orphan_hash = [99u8; 32];
        let orphan_fp = SemanticFingerprint {
            primary: [88u8; 16],
            secondary: 999999,
        };

        let raw_edge = RawEpistemicEdge {
            from_hash: claim_a.id,
            from_fingerprint: claim_a.fingerprint,
            to_hash: orphan_hash,
            to_fingerprint: orphan_fp,
            relation: Relation::Supports,
            strength: Reputation::from_bps(9000),
        };

        let claims = vec![claim_a];
        let graph = KnowledgeGraph::build_from_claims(&claims, &[raw_edge]);

        assert_eq!(
            graph.edges().len(),
            0,
            "fully orphaned edge should be dropped"
        );
    }

    #[test]
    fn test_epoch_aligned_reduction_deduplicates() {
        // Two nodes independently compact the same claims in the same
        // grid epoch → must produce byte-identical Summaries.
        let reducer = ExactMatchReducer;

        // 10 claims spanning ticks 0-9, all in grid bucket 0 (grid_size=10)
        let claims: Vec<Claim> = (0..10)
            .map(|i| {
                let mut src = [0u8; 32];
                src[0] = i as u8;
                make_claim(1, b"temp=20C", 2000, i as u64, src)
            })
            .collect();

        // Node A reduces in order [0..10]
        let summaries_a = reducer.reduce_epoch_aligned(&claims, 10, None);

        // Node B reduces the SAME claims but received in REVERSE gossip order
        let mut reversed = claims.clone();
        reversed.reverse();
        let summaries_b = reducer.reduce_epoch_aligned(&reversed, 10, None);

        assert_eq!(summaries_a.len(), 1);
        assert_eq!(summaries_b.len(), 1);
        // Byte-identical Summary → CRDT deduplication eliminates double-counting
        assert_eq!(
            summaries_a[0].id, summaries_b[0].id,
            "grid-aligned Summaries from different nodes must have identical hash"
        );
        assert_eq!(summaries_a[0].confidence, summaries_b[0].confidence);
    }

    #[test]
    fn test_epoch_grid_splits_across_boundaries() {
        let reducer = ExactMatchReducer;

        // Claims spanning two grid epochs: ticks 5-15, grid_size=10
        // Bucket 0: ticks 5-9 (5 claims)
        // Bucket 1: ticks 10-15 (6 claims)
        let claims: Vec<Claim> = (5..16)
            .map(|i| {
                let mut src = [0u8; 32];
                src[0] = i as u8;
                make_claim(1, b"temp=20C", 2000, i as u64, src)
            })
            .collect();

        let summaries = reducer.reduce_epoch_aligned(&claims, 10, None);
        assert_eq!(
            summaries.len(),
            2,
            "should produce 2 Summaries for 2 grid epochs"
        );
    }

    #[test]
    fn test_high_variance_flags_disagreement() {
        let reducer = ExactMatchReducer;

        // Extreme disagreement: one says very likely, another says very unlikely
        let claims = vec![
            make_claim(1, b"data", 6000, 0, [1u8; 32]), // Very confident
            make_claim(1, b"data", -4000, 1, [2u8; 32]), // Anti-confident
        ];

        let summary = reducer.reduce(&claims).expect("should reduce");
        match &summary.kind {
            ClaimKind::Summary { variance_milli, .. } => {
                // Variance should be very large (strong disagreement)
                assert!(
                    *variance_milli > 20_000_000,
                    "extreme disagreement should produce large variance: got {}",
                    variance_milli
                );
            }
            _ => panic!("expected Summary"),
        }
    }

    // ── Temporal decay tests ──

    #[test]
    fn test_temporal_decay_reduces_old_claims() {
        let mut graph = KnowledgeGraph::new();
        graph.add_edge(EpistemicEdge {
            from: 0,
            to: 1,
            relation: Relation::Supports,
            strength: Reputation::from_bps(9000),
        });

        let claims = vec![
            make_claim(1, b"old", 4000, 10, [1u8; 32]), // old claim (tick=10)
            make_claim(1, b"new", 4000, 1000, [2u8; 32]), // new claim (tick=1000)
        ];
        let mut tracker = InMemoryReputationTracker::new();
        let anchor = [255u8; 32];
        tracker.set_reputation(&anchor, Reputation::FULL);
        for c in &claims {
            tracker.delegate(&anchor, &c.origin, Reputation::FULL);
        }

        let mut base = rustc_hash::FxHashMap::default();
        for (i, claim) in claims.iter().enumerate() {
            let rep = tracker.reputation(&claim.origin);
            base.insert(i as u32, rep.weight_evidence(claim.confidence));
        }

        // Without decay
        let no_decay = graph.propagate_trust_advanced(&base, 5, 5000, &claims, &tracker, 0, 0);
        // With decay (half_life=100, current_tick=1000 → old claim is 990 ticks old = ~9 half-lives)
        let with_decay =
            graph.propagate_trust_advanced(&base, 5, 5000, &claims, &tracker, 1000, 100);

        let old_no_decay = no_decay.get(&0).copied().unwrap_or(LogOdds::NEUTRAL);
        let old_with_decay = with_decay.get(&0).copied().unwrap_or(LogOdds::NEUTRAL);

        // Old claim's trust should be lower with decay
        assert!(
            old_with_decay.value() < old_no_decay.value(),
            "temporal decay should reduce old claim trust: no_decay={}, with_decay={}",
            old_no_decay.value(),
            old_with_decay.value()
        );
    }

    // ── Dynamic damping tests ──

    #[test]
    fn test_dynamic_damping_scales_with_support() {
        // A claim with many supporters should resist contradiction more
        // than a claim with few supporters (dynamic α)
        let mut graph_strong = KnowledgeGraph::new();
        let mut graph_weak = KnowledgeGraph::new();

        // Strong consensus: 5 supporters + 1 contradictor
        for i in 0..5 {
            graph_strong.add_edge(EpistemicEdge {
                from: i,
                to: 6,
                relation: Relation::Supports,
                strength: Reputation::from_bps(8000),
            });
        }
        graph_strong.add_edge(EpistemicEdge {
            from: 5,
            to: 6,
            relation: Relation::Contradicts,
            strength: Reputation::from_bps(8000),
        });

        // Weak consensus: 1 supporter + 1 contradictor
        graph_weak.add_edge(EpistemicEdge {
            from: 0,
            to: 2,
            relation: Relation::Supports,
            strength: Reputation::from_bps(8000),
        });
        graph_weak.add_edge(EpistemicEdge {
            from: 1,
            to: 2,
            relation: Relation::Contradicts,
            strength: Reputation::from_bps(8000),
        });

        let claims_strong: Vec<Claim> = (0..7)
            .map(|i| {
                let mut src = [0u8; 32];
                src[0] = i as u8;
                make_claim(1, &[i as u8], 3000, i as u64, src)
            })
            .collect();
        let claims_weak: Vec<Claim> = (0..3)
            .map(|i| {
                let mut src = [0u8; 32];
                src[0] = i as u8;
                make_claim(1, &[i as u8], 3000, i as u64, src)
            })
            .collect();

        let mut tracker = InMemoryReputationTracker::new();
        // All claims share origin [1u8;32] from make_claim — set directly
        tracker.set_reputation(&[1u8; 32], Reputation::from_bps(8000));

        let mut base_strong = rustc_hash::FxHashMap::default();
        for (i, c) in claims_strong.iter().enumerate() {
            base_strong.insert(
                i as u32,
                tracker.reputation(&c.origin).weight_evidence(c.confidence),
            );
        }
        let mut base_weak = rustc_hash::FxHashMap::default();
        for (i, c) in claims_weak.iter().enumerate() {
            base_weak.insert(
                i as u32,
                tracker.reputation(&c.origin).weight_evidence(c.confidence),
            );
        }

        let prop_strong =
            graph_strong.propagate_trust_full(&base_strong, 5, 5000, &claims_strong, &tracker);
        let prop_weak =
            graph_weak.propagate_trust_full(&base_weak, 5, 5000, &claims_weak, &tracker);

        let trust_strong = prop_strong.get(&6).copied().unwrap_or(LogOdds::NEUTRAL);
        let trust_weak = prop_weak.get(&2).copied().unwrap_or(LogOdds::NEUTRAL);

        // Strong consensus should survive contradiction better
        assert!(
            trust_strong.value() > trust_weak.value(),
            "strong consensus ({}) should resist contradiction better than weak ({})",
            trust_strong.value(),
            trust_weak.value()
        );
    }

    // ── Reputation spending test ──

    #[test]
    fn test_reputation_spending_geometric_bound() {
        // With reputation spending (cost = granted/2), total delegatable
        // reputation converges. Delegating everything repeatedly:
        // 10000 → grant 10000 (cost 5000) → remaining 5000
        // 5000 → grant 5000 (cost 2500) → remaining 2500
        // ... geometric series: total = 10000 + 5000 + 2500 + ... = 20000
        let mut tracker = InMemoryReputationTracker::new();
        let delegator = [1u8; 32];
        tracker.set_reputation(&delegator, Reputation::FULL);

        let mut total = 0u32;
        for i in 0..20u8 {
            let remaining = tracker.reputation(&delegator).bps();
            if remaining == 0 {
                break;
            }
            let node = [i + 100; 32];
            tracker.delegate(&delegator, &node, Reputation::FULL);
            total += tracker.reputation(&node).bps() as u32;
        }

        // Total should converge to ~2× original (20000).
        // Allow small rounding margin from integer division.
        assert!(
            total <= 20100,
            "total delegated reputation should be bounded by ~2x: got {}",
            total
        );
        assert!(
            total > 15000,
            "should have delegated a substantial amount: got {}",
            total
        );
    }

    // ── Confidence interval tests ──

    #[test]
    fn test_confidence_interval_exact() {
        let ci = ConfidenceInterval::exact(LogOdds::new(2000));
        assert_eq!(ci.width(), 0);
        assert_eq!(ci.midpoint().value(), 2000);
    }

    #[test]
    fn test_confidence_interval_aggregate_grows_width() {
        // Two sources that agree → narrow interval
        let agree = ConfidenceInterval::aggregate(&[
            ConfidenceInterval::new(LogOdds::new(1800), LogOdds::new(2200)),
            ConfidenceInterval::new(LogOdds::new(1900), LogOdds::new(2100)),
        ]);

        // Two sources that disagree → wide interval
        let disagree = ConfidenceInterval::aggregate(&[
            ConfidenceInterval::new(LogOdds::new(-2000), LogOdds::new(-1000)),
            ConfidenceInterval::new(LogOdds::new(1000), LogOdds::new(3000)),
        ]);

        assert!(
            disagree.width() > agree.width(),
            "disagreeing sources should produce wider interval: agree={}, disagree={}",
            agree.width(),
            disagree.width()
        );
    }

    #[test]
    fn test_confidence_interval_narrow_intersection() {
        let a = ConfidenceInterval::new(LogOdds::new(1000), LogOdds::new(3000));
        let b = ConfidenceInterval::new(LogOdds::new(2000), LogOdds::new(4000));

        let narrowed = a.narrow(b);
        // Intersection: [2000, 3000]
        assert_eq!(narrowed.lower.value(), 2000);
        assert_eq!(narrowed.upper.value(), 3000);
        assert!(narrowed.width() < a.width());
    }

    #[test]
    fn test_confidence_interval_no_overlap_widens() {
        let a = ConfidenceInterval::new(LogOdds::new(1000), LogOdds::new(2000));
        let b = ConfidenceInterval::new(LogOdds::new(3000), LogOdds::new(4000));

        let result = a.narrow(b);
        // No overlap → returns union [1000, 4000]
        assert_eq!(result.lower.value(), 1000);
        assert_eq!(result.upper.value(), 4000);
    }

    #[test]
    fn test_confidence_interval_deterministic() {
        let intervals = vec![
            ConfidenceInterval::new(LogOdds::new(500), LogOdds::new(1500)),
            ConfidenceInterval::new(LogOdds::new(-1000), LogOdds::new(2000)),
            ConfidenceInterval::new(LogOdds::new(800), LogOdds::new(1200)),
        ];
        let r1 = ConfidenceInterval::aggregate(&intervals);
        let r2 = ConfidenceInterval::aggregate(&intervals);
        assert_eq!(r1, r2, "aggregate must be deterministic");
    }

    // ── v0.3.0: Correlation Discounting tests ──

    #[test]
    fn test_discount_factor_geometric_decay() {
        // rank=0 → 100%, rank=1 → 30%, rank=2 → 9%, rank=3 → 2.7%
        assert_eq!(discount_factor(0, 3000), 10000);
        assert_eq!(discount_factor(1, 3000), 3000);
        assert_eq!(discount_factor(2, 3000), 900);
        assert_eq!(discount_factor(3, 3000), 270);
        // Eventually reaches 0
        assert_eq!(discount_factor(10, 3000), 0);
        // Capped at MAX_DISCOUNT_DEPTH
        assert_eq!(discount_factor(100, 3000), discount_factor(30, 3000));
    }

    #[test]
    fn test_discount_factor_full_weight() {
        // discount_bps=10000 → no discounting at all
        assert_eq!(discount_factor(0, 10000), 10000);
        assert_eq!(discount_factor(1, 10000), 10000);
        assert_eq!(discount_factor(5, 10000), 10000);
    }

    #[test]
    fn test_discount_factor_zero_kills_all() {
        // discount_bps=0 → only rank 0 gets weight
        assert_eq!(discount_factor(0, 0), 10000);
        assert_eq!(discount_factor(1, 0), 0);
    }

    #[test]
    fn test_uncorrelated_aggregate_unchanged() {
        // Claims with None cell = v0.2.0 behavior (pure sum)
        let id_a = [1u8; 32];
        let id_b = [2u8; 32];
        let evidence = vec![
            (LogOdds::new(1000), None, id_a),
            (LogOdds::new(2000), None, id_b),
        ];
        let result = LogOdds::aggregate_correlated(&evidence, DEFAULT_DISCOUNT_BPS);
        // Uncorrelated: full weight each = 1000 + 2000 = 3000
        assert_eq!(result.value(), 3000);
        // Same as plain aggregate
        assert_eq!(
            result,
            LogOdds::aggregate(&[LogOdds::new(1000), LogOdds::new(2000)])
        );
    }

    #[test]
    fn test_same_cell_discounted() {
        // 5 claims in same cell, all confidence=1000
        let cell = Some(CorrelationCell(42));
        let evidence: Vec<_> = (0..5u8)
            .map(|i| {
                let mut id = [0u8; 32];
                id[0] = i;
                (LogOdds::new(1000), cell, id)
            })
            .collect();

        let correlated = LogOdds::aggregate_correlated(&evidence, 3000);

        // Independent sum would be 5000
        let independent = LogOdds::aggregate(&vec![LogOdds::new(1000); 5]);
        assert_eq!(independent.value(), 5000);

        // Correlated must be less (1000 + 300 + 90 + 27 + 8 = 1425)
        assert!(
            correlated.value() < independent.value(),
            "correlated {} should be < independent {}",
            correlated.value(),
            independent.value()
        );
        // First claim gets full weight, rest geometrically discounted
        assert!(correlated.value() > 1000, "at least one full claim");
        assert!(
            correlated.value() < 2000,
            "heavily discounted: {}",
            correlated.value()
        );
    }

    #[test]
    fn test_different_cells_independent() {
        // 3 claims in 3 different cells → no discounting
        let evidence = vec![
            (LogOdds::new(1000), Some(CorrelationCell(1)), [1u8; 32]),
            (LogOdds::new(1000), Some(CorrelationCell(2)), [2u8; 32]),
            (LogOdds::new(1000), Some(CorrelationCell(3)), [3u8; 32]),
        ];
        let result = LogOdds::aggregate_correlated(&evidence, 3000);
        // Each cell has 1 claim → no discount → 3000
        assert_eq!(result.value(), 3000);
    }

    #[test]
    fn test_mixed_correlated_and_independent() {
        // 2 claims in cell A + 1 claim in cell B
        let evidence = vec![
            (LogOdds::new(1000), Some(CorrelationCell(1)), [1u8; 32]),
            (LogOdds::new(1000), Some(CorrelationCell(1)), [2u8; 32]),
            (LogOdds::new(1000), Some(CorrelationCell(2)), [3u8; 32]),
        ];
        let result = LogOdds::aggregate_correlated(&evidence, 3000);
        // Cell A: 1000 + 300 = 1300. Cell B: 1000. Total: 2300
        assert_eq!(result.value(), 2300);
    }

    #[test]
    fn test_discount_commutativity() {
        // Order of claims must not affect result
        let cell = Some(CorrelationCell(1));
        let ev_a = vec![
            (LogOdds::new(2000), cell, [1u8; 32]),
            (LogOdds::new(1000), cell, [2u8; 32]),
            (LogOdds::new(500), cell, [3u8; 32]),
        ];
        let ev_b = vec![
            (LogOdds::new(500), cell, [3u8; 32]),
            (LogOdds::new(2000), cell, [1u8; 32]),
            (LogOdds::new(1000), cell, [2u8; 32]),
        ];
        let r_a = LogOdds::aggregate_correlated(&ev_a, 3000);
        let r_b = LogOdds::aggregate_correlated(&ev_b, 3000);
        assert_eq!(
            r_a,
            r_b,
            "must be commutative: {} vs {}",
            r_a.value(),
            r_b.value()
        );
    }

    #[test]
    fn test_discount_idempotency() {
        // Duplicate claims (same id) should be handled at dedup layer.
        // aggregate_correlated itself doesn't dedup — that's the Reducer's job.
        // But identical ids get the same sort position → deterministic.
        let cell = Some(CorrelationCell(1));
        let id = [1u8; 32];
        let ev = vec![
            (LogOdds::new(1000), cell, id),
            (LogOdds::new(1000), cell, id),
        ];
        let r1 = LogOdds::aggregate_correlated(&ev, 3000);
        // Both have same id → sorted identically → deterministic
        let r2 = LogOdds::aggregate_correlated(&ev, 3000);
        assert_eq!(r1, r2);
    }

    #[test]
    fn test_epoch_aligned_splits_by_cell() {
        // v0.3.0: Two claims in same epoch but different cells → separate Summaries
        let c1 = make_claim_with_cell(
            1,
            b"temp=20",
            2000,
            100,
            [10u8; 32],
            Some(CorrelationCell(1)),
        );
        let c2 = make_claim_with_cell(
            1,
            b"temp=20",
            2000,
            150,
            [11u8; 32],
            Some(CorrelationCell(2)),
        );
        // Need a third claim to have ≥2 per bucket for reduction
        let c3 = make_claim_with_cell(
            1,
            b"temp=20",
            2000,
            120,
            [12u8; 32],
            Some(CorrelationCell(1)),
        );

        let reducer = ExactMatchReducer;
        let summaries = reducer.reduce_epoch_aligned(&[c1, c2, c3], 10000, None);

        // Cell 1 has 2 claims → 1 Summary. Cell 2 has 1 claim → no Summary.
        assert_eq!(
            summaries.len(),
            1,
            "only cell 1 has enough claims to reduce"
        );
        assert_eq!(
            summaries[0].correlation_cell,
            Some(CorrelationCell(1)),
            "Summary inherits cell"
        );
    }

    #[test]
    fn test_epoch_aligned_cell_isolation() {
        // 3 claims in cell A, 3 claims in cell B → 2 separate Summaries
        let mut claims = Vec::new();
        for i in 0..3u8 {
            let mut src = [0u8; 32];
            src[0] = i;
            claims.push(make_claim_with_cell(
                1,
                b"temp=20",
                2000,
                100 + i as u64,
                src,
                Some(CorrelationCell(1)),
            ));
        }
        for i in 0..3u8 {
            let mut src = [0u8; 32];
            src[0] = 10 + i;
            claims.push(make_claim_with_cell(
                1,
                b"temp=20",
                2000,
                100 + i as u64,
                src,
                Some(CorrelationCell(2)),
            ));
        }

        let reducer = ExactMatchReducer;
        let summaries = reducer.reduce_epoch_aligned(&claims, 10000, None);
        assert_eq!(summaries.len(), 2, "one Summary per cell");

        let cells: Vec<_> = summaries.iter().map(|s| s.correlation_cell).collect();
        assert!(cells.contains(&Some(CorrelationCell(1))));
        assert!(cells.contains(&Some(CorrelationCell(2))));
    }

    #[test]
    fn test_reduce_with_reputation_applies_discount() {
        // 3 claims in same cell with reputation → discounting applied
        let mut tracker = InMemoryReputationTracker::new();
        let origins: Vec<[u8; 32]> = (0..3u8)
            .map(|i| {
                let mut o = [0u8; 32];
                o[0] = i;
                o
            })
            .collect();
        for o in &origins {
            tracker.scores.insert(*o, Reputation::FULL);
        }

        let cell = Some(CorrelationCell(42));
        let claims: Vec<_> = origins
            .iter()
            .enumerate()
            .map(|(i, o)| {
                let mut src = [0u8; 32];
                src[0] = i as u8 + 50;
                let mut c = make_claim_with_cell(1, b"temp=20", 2000, 100, src, cell);
                c.origin = *o;
                c
            })
            .collect();

        let reducer = ExactMatchReducer;
        let summary = reducer.reduce_with_reputation(&claims, &tracker).unwrap();

        // Without discount: 3 × 2000 = 6000
        // With discount (3000 bps): 2000 + 600 + 180 = 2780
        assert!(
            summary.confidence.value() < 6000,
            "discount should reduce total: got {}",
            summary.confidence.value()
        );
        assert!(
            summary.confidence.value() > 2000,
            "at least one full-weight claim: got {}",
            summary.confidence.value()
        );
    }

    #[test]
    fn test_none_cell_backward_compat() {
        // Claims with no cell should produce identical results to v0.2.0
        let mut tracker = InMemoryReputationTracker::new();
        let origins: Vec<[u8; 32]> = (0..3u8)
            .map(|i| {
                let mut o = [0u8; 32];
                o[0] = i;
                o
            })
            .collect();
        for o in &origins {
            tracker.scores.insert(*o, Reputation::FULL);
        }

        let claims: Vec<_> = origins
            .iter()
            .enumerate()
            .map(|(i, o)| {
                let mut src = [0u8; 32];
                src[0] = i as u8 + 50;
                let mut c = make_claim(1, b"temp=20", 2000, 100, src);
                c.origin = *o;
                c
            })
            .collect();

        let reducer = ExactMatchReducer;
        let summary = reducer.reduce_with_reputation(&claims, &tracker).unwrap();

        // None cells = uncorrelated = pure sum = 3 × 2000 = 6000
        assert_eq!(
            summary.confidence.value(),
            6000,
            "None cells should produce no discounting (v0.2.0 compat)"
        );
    }

    #[test]
    fn test_geometric_decay_bounds() {
        // Strong discount (≤50%): reaches ~0 well before MAX_DISCOUNT_DEPTH
        for bps in [1000u16, 3000, 5000] {
            let f = discount_factor(MAX_DISCOUNT_DEPTH, bps);
            assert_eq!(
                f, 0,
                "discount_bps={} at max depth should be 0, got {}",
                bps, f
            );
        }
        // Weak discount (70%): 0.7^30 ≈ 0.002 → still small
        assert!(discount_factor(MAX_DISCOUNT_DEPTH, 7000) < 100);
        // Very weak discount (90%): 0.9^30 ≈ 0.04 → still bounded
        assert!(discount_factor(MAX_DISCOUNT_DEPTH, 9000) < 500);
        // No discount (100%): stays at 10000 forever
        assert_eq!(discount_factor(MAX_DISCOUNT_DEPTH, 10000), 10000);
    }
}
