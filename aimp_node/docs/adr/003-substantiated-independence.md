# ADR 003: Independence Must Be Substantiated, Not Asserted

## Status
Accepted

## Context
L3 v0.3.0 introduced Grid-Cell Correlation Discounting to prevent hyper-confidence when correlated sources report concordant observations. It rested on `CorrelationCell`, a field each claim declares about itself, and treated `correlation_cell: None` as "uncorrelated, therefore independent, therefore full weight".

Three problems followed from that, all measured rather than reasoned about (see `aimp_node/tests/insider_asymmetry.rs`):

1. **The incentive ran backwards.** Declaring a cell can only ever *reduce* a source's weight, so a rational participant never declared one. Concealment paid **70.3x** over honest disclosure. The discount was trivially opt-out, and the protocol rewarded misreporting correlation.
2. **The defense never reached the decision path.** `aggregate_correlated` had exactly one production call site, inside `SemanticReducer`. Neither `propagate_trust_advanced` nor `LogOddsBeliefEngine::compute` read `correlation_cell` at all. 100 correlated sources amplified a conclusion to 30,908 where the reducer capped the same 100 at 723. Proven by control: identical topology with same / distinct / absent cells produced byte-identical propagated trust.
3. **Discounting within groups but summing across them bounded nothing.** Any strategy that manufactured K groups earned K times the ceiling.

The root cause is a single conflation: **unverified independence was treated as established independence**. Absence of evidence of correlation is not evidence of absence.

## Decision

Independence is a claim that must be substantiated. Aggregation is two-level (`LogOdds::aggregate_hierarchical`):

- **Level 1** — within a correlation group, geometric discounting as before.
- **Level 2** — group totals are split by whether independence could be *checked*. Groups whose correlation was actually assessed sum at full weight. Groups that merely asserted independence are ranked and discounted against each other, and contribute nothing past `MAX_DISCOUNT_DEPTH`.

Correlation groups are **derived, not trusted** (`correlation_groups()`):

1. Claims carrying a `QuantizedEmbedding` are clustered by exact Hamming connected components. Exact clustering rather than an LSH band because a single 12-bit band has recall ~0.224 at the v0.4.0 support threshold of d=30 — it would miss over three quarters of genuinely correlated pairs.
2. Otherwise the declared cell, hashed into its own domain.
3. Otherwise a bucket derived from the signing origin.

No claim can obtain singleton status by withholding metadata, since step 3 always applies.

Two supporting constraints proved necessary:

- **Content/embedding binding.** The embedding is itself an unvalidated wire field. Without this, K identities asserting byte-identical content while declaring K distant embeddings manufacture K clusters and recover the full 70.3x. Claims sharing normalized content under the same `embedding_version` must declare identical embeddings; disagreement costs the whole content group its assessability. See ADR note below.
- **Keyspace separation.** Cluster keys are `blake3(domain ‖ embedding_version ‖ anchor_claim_id)` — every input public. A raw declared cell could therefore be set to an honest cluster's key, joining that group and demoting it. All three keyspaces are now domain-separated.

## Consequences

**Pros**
- The Bayesian property survives where it is earned: substantiated independent evidence still raises confidence without bound. Defending against manufactured independence by capping *real* independence was rejected for this reason.
- Concealment is no longer profitable, and the residual is flat in N (2,522 at N=100 and at N=1,000).
- Both the reducer and the propagation path now use the same aggregation function, so they cannot drift apart in semantics.

**Cons**
- **BREAKING**: deployments that set no `correlation_cell` and carry no embedding will see aggregate confidence fall. Intended, but it is a real migration cost. See CHANGELOG.
- **`embedding_version` becomes safety-critical.** Two honest builds producing different embeddings for identical content under the same version flag each other and both degrade to unassessed. Fail-closed (no liveness loss), but a failure mode that did not previously exist. Any change capable of altering one output bit — tensor library, quantization, toolchain — requires a version bump. A disagreement observed on a content key is a diagnostic signal of model drift and is worth exposing as a metric.
- **Cost**: clustering is O(N²) — 71 ms for 10k claims in release, against the ~50 ms the v0.4.0 auto-edge pairwise scan already budgets. It runs once per propagation (hoisted out of the fixed-point loop) and once per reducer bucket.
- Single-linkage clustering can chain: a sequence of near pairs can merge claims that are far apart. Tiers are never unioned across each other to keep any chaining confined to embedding-bearing claims.

**Unresolved**
- K independently-delegated identities with distinct evidence sources and no embeddings remain indistinguishable from K independent sources by aggregation alone. Nothing is left to measure. Tracked in issue #10; addressing it requires scoped identity (issue #8), not better arithmetic.

## Verification
Determinism holds under permutation of both edge order and claim arrival order — the latter being the CRDT-relevant one, since gossip delivers in no agreed sequence. Group assignment, aggregate, and propagated trust are all invariant. Propagation converges monotonically to a fixed point despite magnitude-dependent ranking. 136 tests green in debug and in release with `-C overflow-checks=on`.
