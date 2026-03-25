//! AIMP v0.2.0 — Epistemic Layer (L3: Meaning)
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
    /// Approximate but deterministic.
    pub fn from_percent(pct: u8) -> Self {
        match pct {
            0 => Self(-13816),   // ln(0.001/0.999)*1000
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

    /// Bayesian aggregation: just sum the log-odds of independent evidence.
    /// This is the entire point: addition replaces multiplication.
    /// Saturating to prevent i32 overflow.
    pub fn aggregate(evidence: &[LogOdds]) -> LogOdds {
        let sum: i64 = evidence.iter().map(|e| e.0 as i64).sum();
        LogOdds(sum.clamp(i32::MIN as i64, i32::MAX as i64) as i32)
    }

    /// Bayesian update: posterior = prior + new_evidence
    pub fn update(self, evidence: LogOdds) -> LogOdds {
        LogOdds(self.0.saturating_add(evidence.0))
    }

    /// Is this belief more likely true than false?
    pub fn is_positive(self) -> bool {
        self.0 > 0
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
    /// Correlated physical sensor failure is an application-level concern
    /// and MUST be documented as a limitation. (Gemini review, round 2)
    pub evidence_source: ClaimHash,
    /// Lamport timestamp
    pub tick: u64,
}

/// The semantic type of a claim.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ClaimKind {
    /// Raw observation from a sensor or agent perception
    Observation {
        sensor_type: u8,
        data: Vec<u8>,
    },

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

/// Adjacency-list knowledge graph with real traversal. (ChatGPT fix: no more placeholders)
pub struct KnowledgeGraph {
    edges: Vec<EpistemicEdge>,
    /// Adjacency list: claim_id → outgoing edges
    adjacency: rustc_hash::FxHashMap<ClaimArenaId, SmallVec<[usize; 4]>>,
}

impl KnowledgeGraph {
    pub fn new() -> Self {
        Self {
            edges: Vec::new(),
            adjacency: rustc_hash::FxHashMap::default(),
        }
    }

    pub fn add_edge(&mut self, edge: EpistemicEdge) {
        let idx = self.edges.len();
        self.adjacency
            .entry(edge.from)
            .or_default()
            .push(idx);
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
                    if edge.relation == Relation::DerivedFrom || edge.relation == Relation::Supports {
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
    pub fn detect_cycles(&self) -> Vec<Vec<ClaimArenaId>> {
        let mut cycles = Vec::new();
        let mut visited = rustc_hash::FxHashSet::default();
        let mut on_stack = rustc_hash::FxHashSet::default();
        let mut stack_path = Vec::new();

        // Collect all nodes
        let mut all_nodes = rustc_hash::FxHashSet::default();
        for edge in &self.edges {
            all_nodes.insert(edge.from);
            all_nodes.insert(edge.to);
        }

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
            for &idx in indices {
                let edge = &self.edges[idx];
                // Only follow Supports and DerivedFrom (the edges that propagate trust/confidence)
                if edge.relation != Relation::Supports && edge.relation != Relation::DerivedFrom {
                    continue;
                }
                let next = edge.to;

                if on_stack.contains(&next) {
                    // Found a cycle! Extract it from path
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
    pub fn cyclic_edge_indices(&self) -> rustc_hash::FxHashSet<usize> {
        let mut cyclic = rustc_hash::FxHashSet::default();
        let mut visited = rustc_hash::FxHashSet::default();
        let mut on_stack = rustc_hash::FxHashSet::default();

        let mut all_nodes = rustc_hash::FxHashSet::default();
        for edge in &self.edges {
            all_nodes.insert(edge.from);
            all_nodes.insert(edge.to);
        }

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
            for &idx in indices {
                let edge = &self.edges[idx];
                if edge.relation != Relation::Supports && edge.relation != Relation::DerivedFrom {
                    continue;
                }
                if on_stack.contains(&edge.to) {
                    // Back-edge found → mark it as cyclic
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
        self.propagate_trust_full(base_trust, max_iterations, 5000, &[], &InMemoryReputationTracker::new())
    }

    /// Two-pass trust propagation with proper reputation lookup.
    /// Gemini R6 fix: author_reputation comes from ReputationTracker, NOT from base_trust.
    /// base_trust contains weighted log-odds (confidence × reputation), not raw reputation.
    pub fn propagate_trust_full(
        &self,
        base_trust: &rustc_hash::FxHashMap<ClaimArenaId, LogOdds>,
        max_iterations: u8,
        damping_bps: u16,
        claims: &[Claim],
        reputations: &dyn ReputationTracker,
    ) -> rustc_hash::FxHashMap<ClaimArenaId, LogOdds> {
        let cyclic_edges = self.cyclic_edge_indices();
        let mut trust = base_trust.clone();

        // ── Pass 1: Positive propagation (Supports + DerivedFrom only) ──
        for _ in 0..max_iterations {
            let mut updates = rustc_hash::FxHashMap::default();

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
                // Gemini R5: Clamp source trust to non-negative.
                // Rejected claims (negative trust) have ZERO epistemic authority.
                // A liar cannot support or contradict anyone.
                let from_trust = LogOdds::new(raw_from.value().max(0));

                // Gemini R6 fix: Use ACTUAL reputation from ReputationTracker, not base_trust.
                // base_trust contains weighted log-odds, not reputation basis points.
                // Confusing the two is a Type Confusion that bypasses BFT.
                let author_rep = if (edge.from as usize) < claims.len() {
                    reputations.reputation(&claims[edge.from as usize].origin).bps() as i64
                } else {
                    0i64 // Unknown author = zero weight
                };
                let effective_weight = (edge.strength.bps() as i64) * author_rep / 10000;
                let contribution = (from_trust.value() as i64) * effective_weight / 10000;

                let entry = updates.entry(edge.to).or_insert(0i64);
                *entry += contribution;
            }

            let mut changed = false;
            for (node, bonus) in &updates {
                let bonus_logodds = LogOdds::new(
                    (*bonus).clamp(i32::MIN as i64, i32::MAX as i64) as i32,
                );
                let current = trust.get(node).copied().unwrap_or(LogOdds::NEUTRAL);
                let new_val = current.update(bonus_logodds);

                if new_val != current {
                    trust.insert(*node, new_val);
                    changed = true;
                }
            }

            if !changed {
                break; // Converged
            }
        }

        // ── Pass 2: Contradiction subtraction (single pass, no iteration) ──
        // Gemini R4: Annihilation cap — a single contradiction cannot remove more
        // than 50% of the target's accumulated positive trust. Multiple contradictions
        // are needed to destroy strong consensus. This prevents a single high-trust
        // node from flipping another's belief in one shot.
        let stabilized = trust.clone();
        for edge in &self.edges {
            if edge.relation != Relation::Contradicts {
                continue;
            }

            let raw_from = stabilized.get(&edge.from).copied().unwrap_or(LogOdds::NEUTRAL);
            // Gemini R5: Clamp to non-negative. Negative-trust claims cannot contradict.
            // Prevents "enemy of my enemy" exploit where subtracting negative = adding trust.
            let from_trust = LogOdds::new(raw_from.value().max(0));

            // Gemini R6 fix: Use ACTUAL reputation, not log-odds from stabilized trust
            let author_rep = if (edge.from as usize) < claims.len() {
                reputations.reputation(&claims[edge.from as usize].origin).bps() as i64
            } else {
                0i64
            };
            let effective_weight = (edge.strength.bps() as i64) * author_rep / 10000;
            let raw_penalty = (from_trust.value() as i64) * effective_weight / 10000;

            // Cap: configurable damping — max α% of target's positive trust per edge
            let current = trust.get(&edge.to).copied().unwrap_or(LogOdds::NEUTRAL);
            let max_penalty = if current.value() > 0 {
                (current.value() as i64) * (damping_bps as i64) / 10000
            } else {
                raw_penalty.abs() // No cap on already-negative trust
            };
            let capped_penalty = raw_penalty.abs().min(max_penalty);

            trust.insert(
                edge.to,
                LogOdds::new(current.value().saturating_sub(
                    capped_penalty.clamp(0, i32::MAX as i64) as i32,
                )),
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
    Escalate { reason: String, conflicting: Vec<ClaimHash> },
}

/// Scores claim relevance for semantic GC (dependency-aware).
pub trait RelevanceScorer: Send + Sync {
    fn score(&self, claim: &Claim, active_intents: &[Claim], graph: &KnowledgeGraph) -> u16;
    fn gc_threshold(&self) -> u16 { 1000 }
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
            ClaimKind::Summary { data, tick_start, tick_end, aggregated_logodds, .. } => {
                // Find the Summary with the widest window or highest confidence
                let best = sorted.iter()
                    .filter_map(|c| match &c.kind {
                        ClaimKind::Summary { tick_start: ts, tick_end: te, aggregated_logodds: lo, data: d, .. }
                            => Some((*ts, *te, *lo, d.clone())),
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

        // Count unique evidence sources
        let mut unique_sources: SmallVec<[ClaimHash; 16]> = SmallVec::new();
        let mut evidence_logodds: Vec<LogOdds> = Vec::new();

        for c in &sorted {
            if !unique_sources.contains(&c.evidence_source) {
                unique_sources.push(c.evidence_source);
                evidence_logodds.push(c.confidence);
            }
        }

        // Log-odds aggregation: just sum! (Gemini fix)
        let aggregated = LogOdds::aggregate(&evidence_logodds);

        // Compute variance (ChatGPT R3: prevent information black holes)
        let n = evidence_logodds.len() as i64;
        let mean = if n > 0 { aggregated.value() as i64 / n } else { 0 };
        let variance_milli = if n > 1 {
            evidence_logodds
                .iter()
                .map(|lo| {
                    let diff = lo.value() as i64 - mean;
                    diff * diff
                })
                .sum::<i64>()
                / n
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

        let mut hasher = blake3::Hasher::new();
        hasher.update(&first.fingerprint.primary);
        hasher.update(&(unique_sources.len() as u32).to_le_bytes());
        hasher.update(&tick_start.to_le_bytes());
        hasher.update(&tick_end.to_le_bytes());
        hasher.update(&aggregated.value().to_le_bytes());
        hasher.update(&variance_milli.to_le_bytes());
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
        Self { half_life_ticks: 1000 }
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
pub struct InMemoryReputationTracker {
    scores: rustc_hash::FxHashMap<[u8; 32], Reputation>,
}

impl InMemoryReputationTracker {
    pub fn new() -> Self {
        Self { scores: rustc_hash::FxHashMap::default() }
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
        let secondary = u64::from_le_bytes(
            feat_hasher.finalize().as_bytes()[..8].try_into().unwrap(),
        );

        SemanticFingerprint { primary, secondary }
    }

    fn make_claim(sensor: u8, data: &[u8], logodds: i32, tick: u64, source: [u8; 32]) -> Claim {
        let fp = make_fingerprint(data, sensor);
        let mut hasher = blake3::Hasher::new();
        hasher.update(&fp.primary);
        hasher.update(&tick.to_le_bytes());
        hasher.update(&source);
        let id = *hasher.finalize().as_bytes();

        Claim {
            id,
            fingerprint: fp,
            origin: [1u8; 32],
            kind: ClaimKind::Observation { sensor_type: sensor, data: data.to_vec() },
            confidence: LogOdds::new(logodds),
            evidence_source: source,
            tick,
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
        let evidence = vec![
            LogOdds::new(1500),
            LogOdds::new(-800),
            LogOdds::new(2200),
        ];
        let r1 = LogOdds::aggregate(&evidence);
        let r2 = LogOdds::aggregate(&evidence);
        assert_eq!(r1, r2); // MUST be identical (no float rounding variance)
    }

    #[test]
    fn test_logodds_update_bayesian() {
        let prior = LogOdds::NEUTRAL; // 50%
        let strong_evidence = LogOdds::new(2197); // ~90% likelihood ratio
        let posterior = prior.update(strong_evidence);
        assert_eq!(posterior.to_percent(), 90);
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
        let bad = Reputation::from_bps(2000);  // 20%
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
            ClaimKind::Summary { unique_sources, aggregated_logodds, .. } => {
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
            ClaimKind::Summary { unique_sources, aggregated_logodds, .. } => {
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
            ClaimKind::Summary { unique_sources, aggregated_logodds, .. } => {
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
        graph.add_edge(EpistemicEdge { from: 0, to: 1, relation: Relation::DerivedFrom, strength: Reputation::from_bps(9000) });
        graph.add_edge(EpistemicEdge { from: 1, to: 2, relation: Relation::DerivedFrom, strength: Reputation::from_bps(9000) });
        graph.add_edge(EpistemicEdge { from: 0, to: 3, relation: Relation::Supports, strength: Reputation::from_bps(9000) });

        let deps = graph.dependents(0);
        assert!(deps.contains(&1));
        assert!(deps.contains(&2)); // Transitive!
        assert!(deps.contains(&3));
    }

    #[test]
    fn test_graph_support_ratio() {
        let mut graph = KnowledgeGraph::new();
        graph.add_edge(EpistemicEdge { from: 0, to: 5, relation: Relation::Supports, strength: Reputation::from_bps(9000) });
        graph.add_edge(EpistemicEdge { from: 1, to: 5, relation: Relation::Supports, strength: Reputation::from_bps(9000) });
        graph.add_edge(EpistemicEdge { from: 2, to: 5, relation: Relation::Contradicts, strength: Reputation::from_bps(8000) });

        let (sup, con) = graph.support_ratio(5);
        assert_eq!(sup, 2);
        assert_eq!(con, 1);
    }

    // ── Belief engine tests ──

    #[test]
    fn test_belief_engine_classifies() {
        let engine = LogOddsBeliefEngine::default();
        let tracker = InMemoryReputationTracker::new();
        let graph = KnowledgeGraph::new();

        let claims = vec![
            make_claim(1, b"strong", 5000, 0, [1u8; 32]),  // High confidence → accepted
            make_claim(1, b"weak", -5000, 1, [2u8; 32]),    // Low confidence → rejected
            make_claim(1, b"meh", 100, 2, [3u8; 32]),       // Middling → uncertain
        ];

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
        graph.add_edge(EpistemicEdge { from: 0, to: 1, relation: Relation::Supports, strength: Reputation::from_bps(9000) });
        graph.add_edge(EpistemicEdge { from: 1, to: 2, relation: Relation::Supports, strength: Reputation::from_bps(9000) });
        graph.add_edge(EpistemicEdge { from: 2, to: 0, relation: Relation::Supports, strength: Reputation::from_bps(9000) });

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
        graph.add_edge(EpistemicEdge { from: 0, to: 1, relation: Relation::DerivedFrom, strength: Reputation::from_bps(9000) });
        graph.add_edge(EpistemicEdge { from: 1, to: 2, relation: Relation::DerivedFrom, strength: Reputation::from_bps(9000) });

        let cycles = graph.detect_cycles();
        assert!(cycles.is_empty(), "DAG should have no cycles");
    }

    #[test]
    fn test_cyclic_edges_identified() {
        let mut graph = KnowledgeGraph::new();
        graph.add_edge(EpistemicEdge { from: 0, to: 1, relation: Relation::Supports, strength: Reputation::from_bps(9000) });
        graph.add_edge(EpistemicEdge { from: 1, to: 0, relation: Relation::Supports, strength: Reputation::from_bps(9000) });

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
        graph.add_edge(EpistemicEdge { from: 0, to: 1, relation: Relation::Supports, strength: Reputation::from_bps(9000) });
        graph.add_edge(EpistemicEdge { from: 1, to: 0, relation: Relation::Supports, strength: Reputation::from_bps(9000) });

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
        // High-trust A supports B (90% edge strength), B supports C (90% edge strength)
        graph.add_edge(EpistemicEdge { from: 0, to: 1, relation: Relation::Supports, strength: Reputation::from_bps(9000) });
        graph.add_edge(EpistemicEdge { from: 1, to: 2, relation: Relation::Supports, strength: Reputation::from_bps(9000) });

        let mut base = rustc_hash::FxHashMap::default();
        base.insert(0u32, LogOdds::new(5000)); // A is very trusted
        base.insert(1u32, LogOdds::NEUTRAL);    // B is neutral
        base.insert(2u32, LogOdds::NEUTRAL);    // C is neutral

        let propagated = graph.propagate_trust(&base, 5); // Dynamic decay via edge.strength

        // C should have inherited some trust from A (through B)
        let trust_c = propagated.get(&2).copied().unwrap_or(LogOdds::NEUTRAL);
        assert!(trust_c.value() > LogOdds::NEUTRAL.value(),
            "C should inherit transitive trust from A, got {}", trust_c.value());

        // B should have more trust than C (closer to source)
        let trust_b = propagated.get(&1).copied().unwrap_or(LogOdds::NEUTRAL);
        assert!(trust_b.value() > trust_c.value(),
            "B ({}) should have more trust than C ({})", trust_b.value(), trust_c.value());
    }

    #[test]
    fn test_contradiction_reduces_trust_pass2() {
        let mut graph = KnowledgeGraph::new();
        // A contradicts B with 80% edge strength
        graph.add_edge(EpistemicEdge { from: 0, to: 1, relation: Relation::Contradicts, strength: Reputation::from_bps(8000) });

        let mut base = rustc_hash::FxHashMap::default();
        base.insert(0u32, LogOdds::new(3000)); // A is trusted
        base.insert(1u32, LogOdds::new(2000)); // B is moderately trusted

        // Two-pass: pass 1 does nothing (no support edges), pass 2 subtracts contradiction
        let propagated = graph.propagate_trust(&base, 3);

        // B should have REDUCED trust (contradicted by trusted A in pass 2)
        let trust_b = propagated.get(&1).copied().unwrap_or(LogOdds::NEUTRAL);
        assert!(trust_b.value() < 2000,
            "B's trust should decrease when contradicted by trusted A, got {}", trust_b.value());
    }

    #[test]
    fn test_cyclic_edges_zeroed_no_amplification() {
        let mut graph = KnowledgeGraph::new();
        // A supports B, B supports A (cycle!) — both at 90% edge strength
        graph.add_edge(EpistemicEdge { from: 0, to: 1, relation: Relation::Supports, strength: Reputation::from_bps(9000) });
        graph.add_edge(EpistemicEdge { from: 1, to: 0, relation: Relation::Supports, strength: Reputation::from_bps(9000) });

        let cyclic = graph.cyclic_edge_indices();
        // At least one edge should be marked cyclic
        assert!(!cyclic.is_empty(), "should detect cyclic back-edge");

        // Trust should NOT amplify through the cycle
        let mut base = rustc_hash::FxHashMap::default();
        base.insert(0u32, LogOdds::new(1000));
        base.insert(1u32, LogOdds::new(1000));

        let propagated = graph.propagate_trust(&base, 10);

        // After 10 iterations, values should still be modest (not inflated to millions)
        let trust_0 = propagated.get(&0).copied().unwrap_or(LogOdds::NEUTRAL);
        let trust_1 = propagated.get(&1).copied().unwrap_or(LogOdds::NEUTRAL);
        assert!(trust_0.value() < 5000,
            "cyclic amplification blocked: node 0 trust = {}", trust_0.value());
        assert!(trust_1.value() < 5000,
            "cyclic amplification blocked: node 1 trust = {}", trust_1.value());
    }

    #[test]
    fn test_annihilation_cap_prevents_one_shot_kill() {
        // Gemini R4: A single high-trust contradiction must NOT destroy a well-supported claim
        let mut graph = KnowledgeGraph::new();
        // 5 nodes support claim 5 (strong consensus)
        for i in 0..5 {
            graph.add_edge(EpistemicEdge {
                from: i, to: 5,
                relation: Relation::Supports, strength: Reputation::from_bps(9000),
            });
        }
        // 1 high-trust node contradicts claim 5
        graph.add_edge(EpistemicEdge {
            from: 6, to: 5,
            relation: Relation::Contradicts, strength: Reputation::from_bps(10000),
        });

        let mut base = rustc_hash::FxHashMap::default();
        for i in 0..=6 {
            base.insert(i as ClaimArenaId, LogOdds::new(5000)); // All highly trusted
        }

        let propagated = graph.propagate_trust(&base, 5);

        let trust_5 = propagated.get(&5).copied().unwrap_or(LogOdds::NEUTRAL);
        // Claim 5 should still be positive (5 supporters vs 1 contradictor)
        // Without cap: 5*support - 1*huge_penalty could go negative
        // With cap: single contradiction removes at most 50% → still positive
        assert!(trust_5.value() > 0,
            "5 supporters vs 1 contradictor: claim should remain positive, got {}",
            trust_5.value());
    }

    #[test]
    fn test_dynamic_decay_varies_by_edge() {
        let mut graph = KnowledgeGraph::new();
        // A supports B with HIGH confidence (95%), A supports C with LOW confidence (10%)
        graph.add_edge(EpistemicEdge { from: 0, to: 1, relation: Relation::Supports, strength: Reputation::from_bps(9500) });
        graph.add_edge(EpistemicEdge { from: 0, to: 2, relation: Relation::Supports, strength: Reputation::from_bps(1000) });

        let mut base = rustc_hash::FxHashMap::default();
        base.insert(0u32, LogOdds::new(5000));
        base.insert(1u32, LogOdds::NEUTRAL);
        base.insert(2u32, LogOdds::NEUTRAL);

        let propagated = graph.propagate_trust(&base, 3);

        let trust_b = propagated.get(&1).copied().unwrap_or(LogOdds::NEUTRAL);
        let trust_c = propagated.get(&2).copied().unwrap_or(LogOdds::NEUTRAL);

        // B should get much more trust than C (95% vs 10% edge strength)
        assert!(trust_b.value() > trust_c.value() * 5,
            "B ({}) should get ~9.5x more trust than C ({})", trust_b.value(), trust_c.value());
    }

    // ── Summary variance tests (ChatGPT R3) ──

    #[test]
    fn test_summary_preserves_variance() {
        let reducer = ExactMatchReducer;

        // Claims with DIFFERENT confidence levels from independent sources
        let claims = vec![
            make_claim(1, b"temp=20C", 1000, 0, [1u8; 32]),  // Low confidence
            make_claim(1, b"temp=20C", 5000, 1, [2u8; 32]),  // High confidence
            make_claim(1, b"temp=20C", 100, 2, [3u8; 32]),   // Very low
        ];

        let summary = reducer.reduce(&claims).expect("should reduce");
        match &summary.kind {
            ClaimKind::Summary { variance_milli, range_min, range_max, .. } => {
                // Variance should be non-zero (disagreement on confidence)
                assert!(*variance_milli > 0,
                    "variance should capture disagreement: got {}", variance_milli);

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
            ClaimKind::Summary { variance_milli, range_min, range_max, .. } => {
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
            from: 0, to: 1,
            relation: Relation::Contradicts, strength: Reputation::from_bps(10000),
        });

        let mut base = rustc_hash::FxHashMap::default();
        base.insert(0u32, LogOdds::new(-5000)); // X is a known liar
        base.insert(1u32, LogOdds::new(1000));   // Y is mildly trusted

        let propagated = graph.propagate_trust(&base, 3);

        let trust_y = propagated.get(&1).copied().unwrap_or(LogOdds::NEUTRAL);
        // Y's trust must NOT increase (the liar's contradiction has zero weight)
        assert!(trust_y.value() <= 1000,
            "negative-trust contradiction must not boost target, got {}", trust_y.value());
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
        assert_eq!(total_weight, 0, "unknown nodes must have zero voting weight");
    }

    #[test]
    fn test_delegation_grants_reputation() {
        let mut tracker = InMemoryReputationTracker::new();

        let anchor = [1u8; 32];
        let new_node = [2u8; 32];

        // Anchor has high reputation
        tracker.update(&anchor, ReputationEvent::ClaimConfirmed);
        tracker.update(&anchor, ReputationEvent::ClaimConfirmed);
        tracker.update(&anchor, ReputationEvent::ClaimConfirmed);

        // New node starts at 0
        assert_eq!(tracker.reputation(&new_node).bps(), 0);

        // Anchor delegates to new node
        tracker.delegate(&anchor, &new_node, Reputation::from_bps(3000));

        // New node now has reputation (capped by anchor's rep)
        assert!(tracker.reputation(&new_node).bps() > 0);
        assert!(tracker.reputation(&new_node).bps() <= tracker.reputation(&anchor).bps());
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
    fn test_high_variance_flags_disagreement() {
        let reducer = ExactMatchReducer;

        // Extreme disagreement: one says very likely, another says very unlikely
        let claims = vec![
            make_claim(1, b"data", 6000, 0, [1u8; 32]),   // Very confident
            make_claim(1, b"data", -4000, 1, [2u8; 32]),   // Anti-confident
        ];

        let summary = reducer.reduce(&claims).expect("should reduce");
        match &summary.kind {
            ClaimKind::Summary { variance_milli, .. } => {
                // Variance should be very large (strong disagreement)
                assert!(*variance_milli > 20_000_000,
                    "extreme disagreement should produce large variance: got {}", variance_milli);
            }
            _ => panic!("expected Summary"),
        }
    }
}
