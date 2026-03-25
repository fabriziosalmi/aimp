# AIMP v0.2.0 — Cognitive Layer (L3: Meaning)

## Status: Prototype (3 rounds of multi-AI review)

## Architecture Decision
**Separate crate, separate paper.** (Gemini, round 2: "mixing L2 and L3 is academic suicide")

- `aimp-core` (v0.1.0): L1+L2 — FROZEN, published on ResearchGate
- `aimp-cognitive` (v0.2.0): L3 — built ABOVE, never touches L2 internals

## Design Rules

| # | Rule | Source | Why |
|---|------|--------|-----|
| 1 | **Log-Odds (i32), zero floats** | Gemini R2 | IEEE 754 non-reproducible across arch. u16 underflows on Bayesian multiply. Log-odds: update = addition. |
| 2 | **L3 never blocks L2** | All | CRDT merges at 1.28M ops/sec regardless |
| 3 | **Materialized Compaction** | Gemini R2 | L3 re-injects Summaries into L2 as new mutations BEFORE epoch GC. Prevents split-brain dangling pointers. |
| 4 | **Reputation × Confidence** | All | Self-declared confidence alone is worthless. Byzantine with rep=0 → zero influence. |
| 5 | **Evidence provenance (network-level only)** | Gemini R2 | Prevents Sybil amplification. Does NOT solve correlated physical sensor failure (documented limitation). |
| 6 | **Reducer: commutative, associative, idempotent** | Gemini R2 | Otherwise Knowledge Graph diverges across nodes. |
| 7 | **BeliefState** | ChatGPT R2 | Claims classified into accepted/rejected/uncertain. Transforms accumulation into reasoning. |

## Log-Odds Rationale (Gemini Fix)

**Problem**: Linear u16 basis points (0..=10000) cause underflow on recursive Bayesian multiplication. A Byzantine node can exploit rounding to zero-out true facts.

**Solution**: Log-odds = ln(p/(1-p)) × 1000, stored as i32.
- Bayesian update = **addition** (not multiplication)
- Zero underflow (i32 range: ±2 billion)
- 100% deterministic (integer arithmetic only)
- Lookup table for percent conversion (no ln/exp needed)

| LogOdds | Probability |
|---------|-------------|
| -6907 | ~0.1% |
| -2197 | ~10% |
| 0 | 50% |
| +2197 | ~90% |
| +6907 | ~99.9% |

## Materialized Compaction (Gemini Fix)

**Problem**: L2 epoch GC deletes old nodes. L3 Knowledge Graph has edges pointing to deleted nodes → dangling pointers → crash.

**Solution**: Before epoch GC triggers, L3's SemanticReducer produces Summary claims and re-injects them into L2 as **new mutations**. The Summary becomes ground truth in the current epoch. Old nodes can safely die.

```
L3: detect impending GC → reduce claims → produce Summary
    ↓ (inject as new L2 mutation)
L2: receives Summary as normal DagNode → safe to GC old epoch
```

## Convergence Proof (Spectral Radius Lemma)

**Lemma.** After DFS back-edge zeroing, the positive sub-adjacency matrix A of
propagation weights is strictly upper triangular with respect to the DFS
topological ordering. Therefore:

1. All eigenvalues of A are 0, and the spectral radius ρ(A) = 0 < 1.
2. The iterative relaxation t_{k+1} = t_0 + A · t_k converges in at most
   D steps, where D is the maximum depth of the acyclic subgraph.
3. Pass 2 (contradiction subtraction) is a single O(E) pass on stabilized values,
   requiring no iteration.

**Corollary.** The Belief Engine is guaranteed to produce identical results on all
nodes regardless of message arrival order, provided the same claim set and graph
are observed. This satisfies the determinism requirement for BFT consensus.

## Annihilation Cap (Gemini R4)

A single Contradicts edge cannot remove more than 50% of the target node's
accumulated positive trust from Pass 1. This prevents a high-authority node from
inverting consensus in a single operation. Multiple independent contradictions are
required to overcome strong agreement — matching real-world Bayesian intuition
where one dissenting expert cannot override a scientific consensus.

## Sybil Defense (Gemini R5)

New nodes start with reputation 0 (not neutral). This prevents Sybil attacks where
an attacker generates 1000 fresh Ed25519 keys that each inherit default voting weight.

To gain reputation, a node must be **delegated** by an existing trusted node (Web of Trust):
```rust
tracker.delegate(&anchor_pubkey, &new_node_pubkey, initial_reputation);
```

Delegation is capped: a delegator cannot grant more reputation than they have themselves.
Slashed nodes (reputation=0) cannot delegate. This creates a cryptographic trust chain
rooted in anchor nodes.

Paper-ready: "To prevent Sybil amplification in open topologies, base epistemic reputation
requires cryptographic delegation from an established anchor node. Untrusted nodes may submit
claims, but their initial propagation weight is rigidly 0."

## Trust Clamping (Gemini R5)

Claims with negative trust (rejected by the network) have zero epistemic authority.
Before propagating influence on any edge: `effective_source_trust = max(0, source_trust)`.

This prevents the "enemy of my enemy" exploit where a liar's contradiction of a target
would add trust (subtracting a negative = adding).

## Documented Limitations

1. **Echo chamber**: `evidence_source` prevents network-level Sybil amplification (same data relayed by N nodes). It does NOT detect correlated physical sensor failure (two broken sensors reading the same wrong value). This is an application-level concern.

2. **Semantic grouping**: `SemanticFingerprint.primary` is hash-based exact match. Semantically identical claims with different encoding ("20C" vs "twenty degrees") require application-level normalization before hashing. The `secondary` feature key provides fuzzy matching on discrete features.

3. **Reducer convergence**: ExactMatchReducer guarantees commutativity/associativity/idempotency. Custom implementations MUST prove these properties or the Knowledge Graph will diverge.

## Components

```
aimp-cognitive/
├── epistemic.rs          — LogOdds, Reputation, Claim, SemanticFingerprint
├── graph.rs              — KnowledgeGraph, EpistemicEdge, traversal
├── belief.rs             — BeliefEngine, BeliefState (accepted/rejected/uncertain)
├── reducer.rs            — SemanticReducer trait + ExactMatchReducer
├── intent.rs             — IntentResolver + ReputationWeightedResolver
├── contradiction.rs      — ContradictionResolver (support vs contradict weighting)
├── scorer.rs             — RelevanceScorer + DependencyAwareScorer
└── reputation.rs         — ReputationTracker + InMemoryReputationTracker
```

## Test Coverage (27 tests)

| Category | Tests | What they verify |
|----------|-------|-----------------|
| Log-odds math | 4 | Aggregation=addition, no underflow, determinism, Bayesian update |
| Reputation | 2 | Byzantine zero influence, evidence scaling |
| Echo chamber | 2 | Same-source dedup, independent aggregation |
| Reducer invariants | 2 | Commutativity, idempotency (Gemini requirement) |
| Knowledge graph | 2 | Transitive dependents, support/contradiction ratio |
| Belief engine | 2 | Classification, reputation affects belief |
| Intent resolution | 1 | Reputable node beats high-confidence Byzantine |
| **Cycle detection** | **4** | **Simple cycle, no-cycle DAG, cyclic edges identified, inflation prevented via edge zeroing** |
| **Trust propagation** | **5** | **Transitive support, contradiction pass-2, cyclic amplification blocked, dynamic decay varies by edge, two-pass convergence** |
| **Summary variance** | **3** | **Disagreement captured, zero when agreement, extreme flags** |
| **Sign-inversion exploit** | **1** | **Negative-trust contradiction cannot boost target (enemy of enemy blocked)** |
| **Sybil defense** | **2** | **Unknown nodes have zero weight, slashed nodes cannot delegate** |
| **Web of Trust** | **2** | **Delegation grants reputation (capped by delegator's own rep)** |

## Paper Target

**Title**: "Byzantine-Tolerant Belief Aggregation over Merkle-DAGs"
(NOT "Epistemic CRDT" — per Gemini: the reducer is not a semilattice operation)

**Venue**: AAMAS (Autonomous Agents and Multi-Agent Systems) or Decentralized AI workshop

**Key claims**:
1. Log-odds Bayesian aggregation over CRDT DAG (deterministic, no floats)
2. Reputation-weighted consensus resistant to confidence gaming
3. Echo-chamber-resistant evidence aggregation (network-level)
4. Materialized compaction preserving epistemic graph integrity

## Review History

| Round | Reviewer | Key Fix |
|-------|----------|---------|
| R1 | Claude | Initial implementation: Claim, Reducer, Scorer, 8 tests |
| R1 | ChatGPT | Add semantic_key, Knowledge Graph, BeliefState concept, Intent enrichment |
| R1 | Gemini | Kill f32, paper separation, reducer must be semilattice, echo chamber scope |
| R2 | Claude | Log-odds i32, materialized compaction, graph traversal, 17 tests |
| R2 | ChatGPT | SemanticFingerprint dual-key, ContradictionResolver, reproducible Inference |
| R2 | Gemini | Log-odds underflow attack, split-brain GC fix, commutativity proof requirement |
| R3 | ChatGPT | Cycle detection, trust propagation, summary variance, contradiction as first-class |
| R3 | Claude | Implementation of ChatGPT R3 fixes, 27 tests |
| R3 | Gemini | Kill flat penalties → zero cyclic edges. Two-pass propagation (no oscillation). Dynamic decay via edge.strength. Variance approved. |
| R3 | Claude | Gemini R3 rewrite: two-pass engine, dynamic decay, cyclic edge zeroing, 30 tests |
| R4 | Gemini | DFS sufficient (skip Tarjan), edge.weight = strength × reputation, annihilation cap 50%, spectral radius convergence proof |
| R4 | Claude | Final implementation: reputation-gated propagation, annihilation cap, convergence lemma, 32 tests |
| R5 | Gemini | "Enemy of my enemy" sign-inversion exploit, async Summary double-counting, Sybil via default reputation |
| R5 | Claude | Trust clamping max(0), Sybil defense (rep=0 default + Web of Trust delegation), Summary overlap handling, 36 tests |
| R6 | Gemini | Type Confusion: base_trust contains log-odds not reputation (BFT bypass). Ghost Summary: evidence_source [0;32] causes dedup collision. |
| R6 | Claude | propagate_trust_full() takes claims+reputations directly. Summary evidence_source = own id (unique). 36 tests. |
