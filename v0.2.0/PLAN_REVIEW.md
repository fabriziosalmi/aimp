# Paper 2 Rewrite Plan — Bring to Paper 1 Level

**Target**: "Byzantine-Tolerant Belief Aggregation over Merkle-DAGs"
**Benchmark**: "AIMP: AI Mesh Protocol — Design and Evaluation" (Paper 1)
**Status**: Draft — actionable list, not aspirational

---

## 0. Diagnosis: Why Paper 2 Fails

| Criterion | Paper 1 | Paper 2 (current) | Gap |
|---|---|---|---|
| Benchmarks | Criterion micro-bench, 5-node cluster, netem, ARM64, memory wall, Automerge/Yrs comparison | **Zero performance data** | Fatal |
| Formal verification | TLA+ 101M states, 2 real bugs found | Theorem 1 "stated but not machine-checked" | Critical |
| Comparison with SOTA | Automerge v0.7, Yrs v0.25 — measured | None | Fatal |
| Reproducibility | `./benchmarks/run_all.sh` one-command | No script, no instructions | Fatal |
| Validation method | Integration tests + property-based tests + TLC model checking | "6 rounds of adversarial multi-AI review" = dev process, not validation | Credibility-destroying |
| Application domain | Clear: edge agent sync, IoT, disaster response | Vague: "multi-agent belief" with no concrete scenario | Weak |
| Complexity analysis | O(N²×D) sync, O(1) Merkle root, O(log₂N) proofs — measured | None | Critical |

**Root cause**: Paper 2 describes a *design* and presents the *development process* as validation. Paper 1 describes a *system* and presents *measurements* as validation.

---

## 1. Benchmarks (Priority: BLOCKING)

Paper 1 has 8 benchmark categories. Paper 2 has zero. This alone disqualifies it.

### 1.1 Micro-benchmarks (Criterion)

Create `benches/epistemic.rs` with Criterion harnesses for:

| Benchmark | What it measures | Expected baseline |
|---|---|---|
| `logodds_aggregate_N` | `LogOdds::aggregate()` for N=10,100,1000 evidence items | Sub-µs (saturating i32 addition) |
| `logodds_from_percent` | Lookup table conversion | <10 ns |
| `fingerprint_compute` | `SemanticFingerprint` BLAKE3 hash | ~1 µs (matches Paper 1 BLAKE3) |
| `graph_add_edge_N` | Insert N edges into KnowledgeGraph | O(1) amortized push |
| `graph_detect_cycles_N` | DFS cycle detection on N-node graph | O(V+E) |
| `graph_cyclic_edges_N` | Back-edge identification | O(V+E) |
| `trust_propagation_NxE` | Full two-pass propagation on N claims, E edges | Key number for the paper |
| `reducer_exact_match_N` | ExactMatchReducer on N claims | Must show aggregation is cheap |
| `belief_engine_NxE` | Full pipeline: base trust → propagation → classification | End-to-end L3 cost |

**Output format**: Same as Paper 1 — table with Median and Throughput columns.

**Acceptance criterion**: Every number in the paper must come from a Criterion run with `--save-baseline`.

### 1.2 Hot-Path Profiling

Mirror Paper 1 Section 7.2. Profile the belief engine pipeline per-step:

| Step | Expected % |
|---|---|
| Base trust computation (reputation × confidence) | ~5% |
| Pass 1: positive propagation | ~40-60% |
| DFS cycle detection | ~10-20% |
| Pass 2: contradiction subtraction | ~15-25% |
| Classification | ~5% |

This answers: "Where is the bottleneck in L3?" — the equivalent of Paper 1's "Ed25519 is 88.5%."

### 1.3 Scalability Benchmarks

| Configuration | Metric | Why |
|---|---|---|
| 10, 100, 1000, 10000 claims × sparse graph | Propagation latency | Shows O(V+E) empirically |
| 10, 100, 1000, 10000 claims × dense graph | Propagation latency | Shows worst-case |
| 100 claims × increasing cycle density | Cycle detection overhead | Quantifies DFS cost |
| N claims with GC compaction | Summary generation + re-injection time | Validates materialized compaction doesn't block L2 |
| Knowledge graph memory footprint | Bytes per claim, bytes per edge | Matches Paper 1's ~200 bytes/DAG node |

### 1.4 Integration Benchmark: L3 on top of L2

The critical missing measurement: **What is the end-to-end overhead of L3 on a running AIMP cluster?**

| Scenario | What to measure |
|---|---|
| 5-node cluster, 1000 mutations, L3 disabled | Throughput (baseline from Paper 1: 96K mut/s) |
| 5-node cluster, 1000 mutations, L3 enabled | Throughput (must show L3 overhead) |
| 5-node cluster, L3 belief convergence | Time for all nodes to agree on BeliefState after mutations |
| Convergence under partition + merge | L2 converges in 1 round. How many L3 propagation cycles? |

**This is the single most important measurement in the paper**: L3's overhead on L2's performance.

---

## 2. Property-Based Testing (Priority: CRITICAL)

Paper 1 has 2 property-based tests. Paper 2 claims algebraic invariants but tests them with hand-written unit tests only.

### 2.1 Reducer Properties (proptest)

```
// In proptest regression dir: proptest-regressions/epistemic/
#[proptest]
fn reducer_commutativity(claims: Vec<Claim>) {
    let r = ExactMatchReducer;
    // For all permutations of claims, reduce() produces identical output
    // (test random permutation pairs)
}

#[proptest]
fn reducer_associativity(a: Vec<Claim>, b: Vec<Claim>, c: Vec<Claim>) {
    let r = ExactMatchReducer;
    // reduce(reduce(a,b), c) == reduce(a, reduce(b,c))
}

#[proptest]
fn reducer_idempotency(claims: Vec<Claim>) {
    let r = ExactMatchReducer;
    // reduce(claims ++ claims) == reduce(claims)
}
```

### 2.2 Trust Propagation Determinism

```
#[proptest]
fn trust_propagation_deterministic(graph: KnowledgeGraph, claims: Vec<Claim>) {
    // Two runs with same input produce identical output
    // (catches any non-determinism: HashMap iteration order, etc.)
}

#[proptest]
fn trust_propagation_order_independent(graph: KnowledgeGraph, claims: Vec<Claim>) {
    // Different claim arrival orders produce identical final BeliefState
    // (this is the BFT determinism claim from Theorem 1 Corollary)
}
```

### 2.3 Log-Odds Arithmetic

```
#[proptest]
fn logodds_aggregate_commutative(a: LogOdds, b: LogOdds) {
    assert_eq!(LogOdds::aggregate(&[a, b]), LogOdds::aggregate(&[b, a]));
}

#[proptest]
fn logodds_no_overflow_panic(values: Vec<i32>) {
    // Saturating arithmetic never panics
    let logodds: Vec<LogOdds> = values.into_iter().map(LogOdds::new).collect();
    let _ = LogOdds::aggregate(&logodds); // must not panic
}
```

**Acceptance criterion**: `cargo test` runs all property-based tests with at least 256 cases. Proptest regressions committed to repo.

---

## 3. SOTA Comparison (Priority: CRITICAL)

Paper 1 compares against Automerge and Yrs with measured numbers. Paper 2 compares against nothing.

### 3.1 Frameworks to Compare

| Framework | What it does | Why compare |
|---|---|---|
| **Subjective Logic** (Jøsang 2001, 2016) | Belief + disbelief + uncertainty + base rate tuples. Bayesian via Beta distributions. | Most-cited belief aggregation framework. Direct competitor. |
| **Dempster-Shafer Theory** | Belief functions over power set. Dempster's rule of combination. | Classic alternative to Bayesian. Well-known pathological cases. |
| **TrustChain** (Otte et al. 2009) | Decentralized trust propagation on P2P graphs | Direct competitor for trust propagation |
| **EigenTrust** (Kamvar et al. 2003) | Reputation aggregation in P2P (PageRank-like) | Canonical reputation system to compare against |

### 3.2 Comparison Axes

| Axis | L3 (AIMP) | Subjective Logic | Dempster-Shafer |
|---|---|---|---|
| Determinism | ✓ (integer log-odds) | ✗ (IEEE 754 floats) | ✗ (IEEE 754 floats) |
| BFT-compatible | ✓ (identical results on all nodes) | ✗ (floating-point divergence) | ✗ |
| CRDT integration | ✓ (native: L3 on L2) | ✗ (external) | ✗ (external) |
| Cycle handling | ✓ (DFS back-edge zeroing) | Limited (discount factors) | N/A |
| Sybil resistance | ✓ (Web of Trust + reputation=0 default) | ✗ (no identity model) | ✗ |
| Open question conflicts | Via Contradiction Damping | Fusion operator | Dempster's rule (known pathological) |
| Implementation | Rust, O(V+E) | Java (SINTEF lib), O(?) | Various, O(2^|Θ|) |

### 3.3 Quantitative Comparison (new benchmarks)

Implement Subjective Logic and Dempster-Shafer in Rust (minimal, benchmarkable):

| Benchmark | L3 | Subjective Logic | Dempster-Shafer |
|---|---|---|---|
| Aggregation of N evidence items (N=10,100,1000) | saturating i32 add | f64 Beta fusion | f64 power-set combination |
| Trust propagation on 100-node graph | Two-pass O(V+E) | Discount + consensus | N/A (no trust model) |
| Determinism: run on ARM64 vs x86_64 | bit-identical | may differ | may differ |
| BFT consensus: 5 nodes, same input | identical BeliefState | potentially divergent | potentially divergent |

**This is the strongest argument in the paper**: L3 is the only belief aggregation framework designed for BFT consensus on CRDTs. The comparison makes this concrete.

---

## 4. Application Case: Credential Revocation (P4)

Paper 1 has a clear application domain (edge agent sync). Paper 2 needs one.

### 4.1 The Problem (from P4 analysis)

Multi-hop delegation chains:
```
user → orchestrator → sub-agent → tool → CDN edge
```

Every existing protocol propagates authorization forward only. Revocation propagation is either pull-based (OAuth RFC 7009, ~100ms latency) or coarse-grained (Privacy Pass batch revocation).

### 4.2 L3 as P4

Map P4 directly to L3 constructs:

| P4 Concept | L3 Construct | Data |
|---|---|---|
| Grant credential | `Claim::Observation` | `{action_scope, valid_until, user_pubkey}` |
| Delegation | `Relation::DerivedFrom` edge | From parent credential to child |
| Revocation | `Claim` + `Relation::Contradicts` | Points to revoked grant |
| Scope check | `BeliefState::Accepted` | Credential claim has positive belief |
| Trust chain | `ReputationTracker.delegate()` | Web of Trust = OAuth trust chain |

### 4.3 Benchmark: Revocation Propagation

| Scenario | What to measure | Target |
|---|---|---|
| 5-node mesh, credential granted | Grant propagation time (L2 gossip + L3 classification) | <10ms |
| 5-node mesh, credential revoked | Revocation propagation to all nodes | <10ms |
| 20-node mesh, 3-hop delegation chain | Revocation at root → all delegates see Rejected | <100ms |
| 100-node mesh, revocation under 30% packet loss | Convergence rounds for all nodes to reject | Matches Paper 1 netem results |
| Comparison: OAuth RFC 7009 revocation poll | Latency for downstream to discover revocation | ~100ms (measured) |
| Comparison: OCSP stapling lag | Typical revocation delay | ~3600s (documented) |

**Key paper claim**: L3 achieves sub-100ms revocation propagation in a 100-node mesh — orders of magnitude faster than OAuth polling (100ms per-hop, multiplicative) and OCSP stapling (~1h lag). This is a measurable, falsifiable, useful result.

### 4.4 How to Implement

1. Create `examples/bench_revocation.rs` analogous to `examples/bench_convergence.rs`
2. Spin up N in-process AIMP nodes with L3 enabled
3. Node 0 issues a credential (L2 mutation + L3 Claim)
4. Wait for convergence (all nodes: BeliefState::Accepted)
5. Node 0 issues revocation (L2 mutation + L3 Contradicts edge)
6. Measure time until all nodes: BeliefState::Rejected
7. Repeat under netem conditions (loss, latency, jitter)

---

## 5. Formal Verification (Priority: HIGH)

### 5.1 Machine-Check Theorem 1

Current state: "The convergence lemma is stated but not machine-checked."

Options (in order of effort):

| Approach | Effort | Credibility |
|---|---|---|
| **TLA+ spec of two-pass algorithm** | Medium (extend existing `AimpCrdtConvergence.tla`) | High — matches Paper 1's methodology |
| Coq/Lean proof of spectral radius lemma | High | Very high but overkill for this paper |
| Exhaustive test over bounded graphs | Low | Acceptable if documented as bounded model checking equivalent |

**Recommended**: TLA+ specification. Add to `formal/AimpBeliefConvergence.tla`:

Properties to verify:
- **BeliefDeterminism**: Same claim set + same graph → identical BeliefState on all nodes
- **NoOscillation**: Trust values are monotonically non-increasing after Pass 1 converges
- **ContradictionSafety**: A single Contradicts edge cannot flip a claim from Accepted to Rejected (damping cap)

Run TLC with 3 nodes, 3 claims, 3 edges. Report states explored and wall-clock time.

### 5.2 Complexity Analysis (formal, per-component)

Every algorithm must have complexity stated and justified:

| Component | Time Complexity | Space Complexity | Justification |
|---|---|---|---|
| `LogOdds::aggregate(N)` | O(N) | O(1) | Linear scan, saturating add |
| `KnowledgeGraph::detect_cycles()` | O(V+E) | O(V) | Standard DFS |
| `KnowledgeGraph::cyclic_edge_indices()` | O(V+E) | O(V+E) | DFS + back-edge set |
| `propagate_trust_full()` Pass 1 | O(D×E) where D = max depth | O(V) | D iterations, E edges per iteration. D ≤ V for DAG |
| `propagate_trust_full()` Pass 2 | O(E) | O(V) | Single pass over Contradicts edges |
| `ExactMatchReducer::reduce(N)` | O(N log N) | O(N) | Sort by id + dedup + linear scan |
| `BeliefEngine::compute()` | O(V + D×E) | O(V+E) | Base trust O(V) + propagation O(D×E) + classify O(V) |

---

## 6. Structural Rewrite

### 6.1 Remove "Multi-AI Review" as Methodology

**Current**: Section 11 ("Evaluation") and the abstract present "6 rounds of adversarial multi-AI review" as the validation methodology. This is a development process. Presenting it as validation undermines credibility.

**Fix**:
- Move the review history to **Acknowledgments**: "The design benefited from iterative review by Claude, Gemini, and ChatGPT, which identified 13 vulnerability classes subsequently patched."
- Replace Section 11 with **empirical evaluation**: benchmarks, property-based tests, SOTA comparison, and P4 case study.

### 6.2 New Paper Structure

```
§1  Introduction (keep, strengthen with P4 motivation)
§2  Background and Related Work (expand: add Subjective Logic, Dempster-Shafer,
    EigenTrust, TrustChain. Add LDP cite re: noisy provenance degradation)
§3  System Architecture (keep — L1/L2/L3 stack)
§4  Log-Odds Bayesian Aggregation (keep — solid math)
§5  Trust Propagation (keep — two-pass algorithm, Theorem 1)
§6  Sybil Defense (keep — Web of Trust)
§7  Materialized Compaction (keep — GC solution)
§8  Belief Engine (keep — classification)
§9  Application: Credential Revocation Propagation (NEW — P4 case study)
§10 Formal Verification (NEW — TLA+ or exhaustive bounded check of Theorem 1)
§11 Performance Evaluation (NEW — all benchmarks from §1 of this plan)
    §11.1 Micro-benchmarks (Criterion)
    §11.2 Hot-path profiling
    §11.3 Scalability
    §11.4 L3 overhead on L2
    §11.5 SOTA comparison (Subjective Logic, Dempster-Shafer)
    §11.6 P4 revocation benchmark
§12 Security Analysis (keep the 13-vulnerability table, but as structured analysis
    not "review rounds")
§13 Limitations and Future Work (keep, expand with honest P4 deployment gaps)
§14 Conclusion
```

### 6.3 Abstract Rewrite Direction

Remove:
- "The design was produced through 6 rounds of adversarial multi-AI review"
- "yielding 36 unit tests and patches for 13 distinct vulnerability classes"

Add:
- Performance numbers (trust propagation latency, L3 overhead %)
- SOTA comparison result (deterministic vs non-deterministic)
- P4 application result (revocation propagation latency vs OAuth/OCSP)
- TLA+ verification result (states explored, properties verified)

### 6.4 References to Add

```yaml
josang2001:
  title: "A Logic for Uncertain Probabilities"
  author: Jøsang, Audun
  date: 2001
  # Subjective Logic foundational paper

josang2016:
  title: "Subjective Logic: A Formalism for Reasoning Under Uncertainty"
  author: Jøsang, Audun
  date: 2016
  publisher: Springer
  # Subjective Logic textbook

shafer1976:
  title: "A Mathematical Theory of Evidence"
  author: Shafer, Glenn
  date: 1976
  publisher: Princeton University Press
  # Dempster-Shafer foundational work

kamvar2003:
  title: "The EigenTrust Algorithm for Reputation Management in P2P Networks"
  author: [Kamvar, Sepandar D.; Schlosser, Mario T.; Garcia-Molina, Hector]
  date: 2003
  # EigenTrust — canonical P2P reputation comparison

otte2009:
  title: "TrustChain: Trust Management in Decentralized Environments"
  # Or equivalent trust propagation baseline

rfc9576:
  title: "The Privacy Pass Architecture"
  date: 2023
  # Privacy Pass — for P4 context

rfc8693:
  title: "OAuth 2.0 Token Exchange"
  date: 2020
  # Agent delegation — P2 isomorphism

rfc7009:
  title: "OAuth 2.0 Token Revocation"
  date: 2013
  # Revocation baseline for P4 comparison

prakash2026:
  title: "LDP: An Identity-Aware Protocol for Multi-Agent LLM Systems"
  author: Prakash, S.
  date: 2026
  # Noisy provenance degradation result
```

---

## 7. Reproducibility (Priority: BLOCKING)

### 7.1 Benchmark Script

Create `benchmarks/run_epistemic.sh`:

```bash
#!/bin/bash
# Run all L3 benchmarks — mirrors benchmarks/run_all.sh for L2

echo "=== L3 Micro-benchmarks (Criterion) ==="
cargo bench --bench epistemic -- --save-baseline l3

echo "=== L3 Hot-path profiling ==="
cargo run --release --example profile_epistemic

echo "=== L3 Scalability ==="
cargo run --release --example bench_belief_scale

echo "=== L3 overhead on L2 ==="
cargo run --release --example bench_l3_overhead

echo "=== SOTA comparison ==="
cargo run --release --example compare_subjective_logic

echo "=== P4 revocation benchmark ==="
cargo run --release --example bench_revocation

echo "=== Property-based tests ==="
cargo test --test epistemic_proptests -- --test-threads=1
```

### 7.2 Results Output

Mirror Paper 1's `benchmarks/results/` structure:

```
benchmarks/results/
├── epistemic_criterion_raw.txt
├── epistemic_hotpath_raw.txt
├── epistemic_scale_raw.txt
├── l3_overhead_raw.txt
├── subjective_logic_comparison.txt
├── revocation_raw.txt
└── proptest_summary.txt
```

---

## 8. Implementation Checklist

### Phase 1: Infrastructure (before any writing)

- [ ] Create `benches/epistemic.rs` with Criterion harnesses
- [ ] Create `examples/profile_epistemic.rs` for hot-path profiling
- [ ] Create `examples/bench_belief_scale.rs` for scalability
- [ ] Create `examples/bench_l3_overhead.rs` for L3-on-L2 overhead
- [ ] Create `examples/bench_revocation.rs` for P4 case study
- [ ] Create `examples/compare_subjective_logic.rs` for SOTA comparison
- [ ] Create `tests/epistemic_proptests.rs` with proptest cases
- [ ] Create `benchmarks/run_epistemic.sh`
- [ ] Run all, collect baseline numbers

### Phase 2: Verification (after benchmarks exist)

- [ ] Write `formal/AimpBeliefConvergence.tla` (or exhaustive bounded test)
- [ ] Run TLC, record states explored
- [ ] Document any bugs found (if any — be honest)

### Phase 3: Paper (after all data exists)

- [ ] Restructure paper per §6.2
- [ ] Write §9 (P4 application) with measured numbers
- [ ] Write §10 (formal verification) with TLC results
- [ ] Write §11 (performance evaluation) with all benchmark tables
- [ ] Rewrite abstract with actual numbers
- [ ] Expand §2 with SOTA comparison discussion
- [ ] Move "multi-AI review" to acknowledgments
- [ ] Add new references
- [ ] Update `v0.2.0/references.yml`

### Phase 4: Validation

- [ ] `./benchmarks/run_epistemic.sh` runs clean on fresh clone
- [ ] `cargo test` passes all 36+ unit tests + proptest
- [ ] Paper compiles: `typst compile v0.2.0/paper.typ`
- [ ] Every number in the paper has a traceable source in `benchmarks/results/`
- [ ] No claim without measurement or formal proof

---

## 9. What to Kill

These elements from the current Paper 2 must be removed or radically reworked:

| Element | Action | Reason |
|---|---|---|
| "6 rounds of adversarial multi-AI review" as methodology | Move to Acknowledgments | Development process ≠ validation |
| "36 unit tests" as evidence | Replace with benchmark + proptest counts | Unit test count is not a contribution |
| Table 8 (vulnerability classes by "round") | Restructure as security analysis, drop round numbers | "R1", "R2" etc. look like a blog post |
| "Adversarial Multi-AI Review" as section heading | Delete section | Replace with empirical evaluation |
| "The design was forged through an unusual process" (conclusion) | Rewrite | Self-congratulatory tone |
| Theorem 1 "stated but not machine-checked" | Either verify or downgrade to Conjecture | Cannot claim a theorem without proof |

---

## 10. Success Criteria

Paper 2 is at Paper 1's level when:

1. **Every claim has evidence.** No "we believe", no "we expect". Numbers or proofs.
2. **A skeptical reviewer can reproduce.** One command: `./benchmarks/run_epistemic.sh`
3. **SOTA comparison exists.** Reader knows why L3 over Subjective Logic.
4. **Application case is concrete.** P4 revocation with measured latency.
5. **Formal property is verified.** TLA+ or exhaustive bounded check.
6. **The paper reads like a systems paper**, not a design document.
