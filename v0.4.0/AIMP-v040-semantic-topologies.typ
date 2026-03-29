#set document(
  title: "Deterministic Semantic Topologies for Decentralized AI",
  author: "Fabrizio Salmi",
)
#set page(margin: (x: 2.5cm, y: 2.5cm), numbering: "1")
#set text(font: "New Computer Modern", size: 10pt)
#set par(justify: true, leading: 0.65em)
#set heading(numbering: "1.")

#align(center)[
  #text(size: 16pt, weight: "bold")[
    Deterministic Semantic Topologies:\
    BFT-Safe Embedding Quantization for Autonomous\
    Knowledge Graph Assembly over Merkle-DAGs
  ]

  #v(0.5em)
  #text(size: 11pt)[Fabrizio Salmi]
  #v(0.3em)
  #text(size: 9pt, style: "italic")[
    Independent Researcher
  ]
  #v(0.3em)
  #text(size: 9pt)[
    March 2026 --- Technical Report / Preprint
  ]
]

#v(1em)

#block(fill: luma(245), inset: 12pt, radius: 4pt)[
  *Abstract.* The AIMP Epistemic Layer (L3) provides BFT-deterministic
  belief aggregation over Merkle-DAGs, with correlation-aware sensor
  fusion (v0.3.0) and two-pass Markovian trust propagation (v0.2.0).
  However, the epistemic knowledge graph --- the Supports and Contradicts
  edges that drive trust flow --- must be constructed manually by the
  application. This paper eliminates that requirement.

  We introduce _Deterministic Semantic Topologies_: a protocol extension
  that automatically generates epistemic edges from 256-bit SimHash
  embeddings using Hamming distance. Claims carry an optional quantized
  embedding computed application-side from a protocol-mandated canonical
  embedding model. At each epoch boundary, the protocol computes pairwise
  Hamming distances in a deterministic batch (sorted by claim ID),
  emitting Supports edges for close pairs ($d <= 30$ bits) and
  Contradicts edges for distant pairs ($d >= 200$ bits). Edge strength
  is a linear integer function of distance.

  The design introduces three mechanisms: (1) _embedding versioning_,
  allowing protocol-level model upgrades without breaking existing claims;
  (2) a _max-$k$-nearest cap_ that bounds edge density at $O(N dot k)$
  instead of $O(N^2)$; and (3) a _dead zone_ between thresholds that
  prevents spurious edges from semantically ambiguous pairs.

  Hamming distance on 256-bit vectors executes in $tilde 1$ ns via
  hardware popcount. For 1,000 claims per epoch, the full batch
  computation adds $< 0.5$ ms --- negligible compared to trust
  propagation (59% of the v0.2.0 hot path at 140 µs). All arithmetic
  is integer-only (XOR + popcount + basis-point scaling). No floats
  in the protocol core. ZK-circuit-compatible.

  The implementation adds a new `semantic_topology` module (200 lines)
  and two fields to the Claim struct. All 64 existing tests pass
  unchanged; 17 new tests cover Hamming correctness, edge generation
  determinism, version isolation, $k$-cap enforcement, and strength
  gradients. The implementation is open-source (Rust, MIT license).
]

= Introduction

Knowledge graphs in centralized systems --- Google Knowledge Graph,
Wikidata, DBpedia --- rely on curated ETL pipelines to establish
relationships between entities. A team of engineers decides that
"Paris is-capital-of France" and writes the edge. In decentralized
multi-agent systems, there is no such team. Autonomous agents produce
claims about the world (sensor readings, LLM inferences, intents),
and the system must _discover_ the relationships between them
without human intervention.

The AIMP Epistemic Layer (L3) #cite(label("salmi2026l3")) provides the
machinery to _process_ a knowledge graph: two-pass trust propagation,
contradiction damping, Sybil-resistant reputation, and correlation-aware
discounting #cite(label("salmi2026corr")). But the graph itself ---
the Supports and Contradicts edges that drive trust flow --- must be
supplied by the application. This creates a deployment bottleneck:
every AIMP application needs a custom edge-generation policy.

We close this gap with _Deterministic Semantic Topologies_: a protocol
extension that automatically assembles the knowledge graph from
quantized semantic embeddings. The key insight is that Locality-Sensitive
Hashing (SimHash) #cite(label("charikar2002")) compresses dense floating-point
embeddings into compact binary signatures where semantic distance
reduces to Hamming distance --- an operation that is integer-only,
deterministic, and executes in $tilde 1$ ns via hardware popcount.

== Contributions

+ *Quantized Semantic Distance.* We define `QuantizedEmbedding([u64; 4])`,
  a 256-bit SimHash stored as 4 machine words. Hamming distance between
  two embeddings approximates cosine similarity:
  $"sim" approx (256 - d_H) / 256$. The computation is XOR + popcount,
  requiring zero floating-point operations in the protocol core.

+ *Autonomous Edge Generation.* At each epoch boundary, the protocol
  sorts claims by ID and computes pairwise Hamming distances. Pairs
  below a support threshold produce Supports edges; pairs above a
  contradiction threshold produce Contradicts edges. The batch is
  deterministic: two nodes with the same claims produce identical
  edge sets.

+ *Embedding Versioning.* Claims carry an `embedding_version: u32`
  field. Only claims with matching versions are compared. This allows
  the protocol to upgrade its canonical embedding model without
  invalidating existing claims or breaking CRDT convergence.

+ *Bounded Edge Density.* A per-claim `max_k_nearest` cap limits the
  number of auto-generated edges, bounding the knowledge graph to
  $O(N dot k)$ edges instead of $O(N^2)$.

== Relationship to Prior Work

This paper builds on the AIMP protocol stack:
- *L1/L2 (v0.1.0)*: Merkle-DAG CRDT with Ed25519 signing
  #cite(label("salmi2026aimp")).
- *L3 (v0.2.0)*: Epistemic Layer with integer log-odds and trust
  propagation #cite(label("salmi2026l3")).
- *L3 (v0.3.0)*: Correlation-aware aggregation with geometric
  discounting #cite(label("salmi2026corr")).

SimHash was introduced by Charikar #cite(label("charikar2002")) as a
dimensionality reduction technique preserving cosine similarity.
Locality-Sensitive Hashing (LSH) for approximate nearest neighbors
was formalized by Indyk and Motwani #cite(label("indyk1998")). Modern
sentence embedding models #cite(label("reimers2019")) #cite(label("wang2022"))
produce dense vectors that SimHash can compress to binary signatures.

The novelty is not SimHash itself, but its integration into a
BFT-deterministic CRDT belief aggregation protocol --- where the
hash must produce identical results across architectures and the
resulting graph must satisfy strict algebraic properties.

= The Canonical Latent Space

== The Cross-Model Alignment Problem

Different LLM families (Llama, Mistral, GPT) produce embeddings in
topologically incompatible latent spaces. A cosine similarity of 0.9
between two Llama-3 embeddings is meaningful; the same cosine
similarity between a Llama-3 and a Mistral-7B embedding is not.

Runtime alignment (Procrustes rotation, CCA) requires floating-point
linear algebra and produces architecture-dependent results ---
incompatible with BFT determinism.

== Protocol-Mandated Reference Model

We solve this by separating _thinking_ from _communicating_. An agent
may use any LLM internally for reasoning. Before emitting a Claim to
the network, the agent passes the claim text through a protocol-mandated
_canonical embedding model_ and computes the SimHash. The protocol
specifies:

- The canonical model (e.g., all-MiniLM-L6-v2, ~20MB)
- The hyperplane matrix (256 random vectors, deterministically derived
  from `BLAKE3("AIMP_L3_HYPERPLANES_V" || version)`)
- The embedding version number (`embedding_version: u32`)

L3 never executes the embedding model. It receives pre-computed
`[u64; 4]` values and compares them via Hamming distance. The float
boundary is on the application side, outside the protocol core.

== Embedding Version Isolation

Claims carry an `embedding_version` field. The `AutoEdgeGenerator`
skips pairs with mismatched versions:

```rust
if c1.embedding_version != c2.embedding_version {
    continue;
}
```

This enables protocol-level model upgrades: when the community adopts
a new canonical model (v2), new claims carry `embedding_version = 2`.
Old claims (v1) continue to function --- they just don't generate
auto-edges with v2 claims. The knowledge graph naturally partitions
by version, and the trust propagation operates correctly on each
partition independently.

= Deterministic Semantic Distance

== SimHash Construction (Application-Side)

The agent computes the 256-bit SimHash as follows:

+ Run claim text through the canonical embedding model, producing
  a dense vector $bold(v) in RR^d$.
+ For each of 256 hyperplanes $bold(h)_i$ (from the protocol's
  deterministic hyperplane matrix):
  - Compute the dot product $bold(v) dot bold(h)_i$.
  - Set bit $i$ to 1 if the dot product is positive, 0 otherwise.
+ Pack the 256 bits into `[u64; 4]`.

This is the _only_ step that involves floating-point arithmetic,
and it occurs on the agent side, _before_ the claim is signed with
Ed25519 and enters the CRDT gossip network.

== Hamming Distance (Protocol-Side)

The protocol computes semantic distance as Hamming distance:

```rust
pub fn hamming_distance(&self, other: &Self) -> u32 {
    self.0.iter().zip(other.0.iter())
        .map(|(a, b)| (a ^ b).count_ones())
        .sum()
}
```

Properties:
- *Range*: $[0, 256]$. Distance 0 = identical meaning. Distance 256 =
  perfectly opposite.
- *Approximation*: $"cosine_similarity" approx (256 - d_H) / 256$
  #cite(label("charikar2002")).
- *Performance*: XOR + popcount. $tilde 1$ ns on modern CPUs with
  hardware `POPCNT` instruction.
- *Determinism*: Integer-only. Identical on ARM64, x86\_64, WASM.
- *ZK-compatible*: XOR and popcount are trivially expressible as
  arithmetic circuits.

== Threshold Policy

Given Hamming distance $d$ between two claims:

#table(
  columns: (auto, auto, auto),
  inset: 6pt,
  stroke: 0.5pt,
  [*Condition*], [*Action*], [*Default*],
  [$d <= T_"support"$], [Emit Supports edge], [30 bits],
  [$T_"support" < d < T_"contradict"$], [No edge (dead zone)], [---],
  [$d >= T_"contradict"$], [Emit Contradicts edge], [200 bits],
)

The _dead zone_ (31--199 bits) prevents spurious edges from
semantically ambiguous pairs. At 128 bits (random chance), no
relationship is asserted.

== Edge Strength Function

Edge strength scales linearly with distance in basis points:

*Supports* ($d <= T_"support"$):
$ "strength"_"bps" = "clamp"(10000 - d times 9000 / T_"support", quad 1000, quad 10000) $

Distance 0 produces strength 10000 (maximum confidence). Distance
$T_"support"$ produces strength 1000 (minimum). This gradient feeds
into the Markovian flow normalization (v0.2.0): stronger edges
carry proportionally more trust.

*Contradicts* ($d >= T_"contradict"$):
$ "strength"_"bps" = "clamp"(1000 + (d - T_"contradict") times 9000 / (256 - T_"contradict"), quad 1000, quad 10000) $

Distance $T_"contradict"$ produces strength 1000. Distance 256
(bitwise opposite) produces strength 10000.

= Automatic Edge Generation Pipeline

== Epoch-Batch Generation

Edge generation is performed as a deterministic batch at each epoch
boundary, alongside the existing grid-aligned reduction (v0.3.0).

```
Input:  claims[] in current epoch
Output: auto-generated RawEpistemicEdge[]

1. Filter: keep only claims with embedding != None.
2. Sort by claim ID (BFT determinism).
3. For each pair (i, j) where i < j:
   a. Skip if embedding_version mismatch.
   b. Compute Hamming distance.
   c. If d <= T_support: emit Supports edge with strength(d).
   d. If d >= T_contradict: emit Contradicts edge with strength(d).
   e. Skip if either claim has reached max_k_nearest edges.
4. Return edges (deterministic order: sorted by from_hash, to_hash).
```

== Determinism Proof

*Theorem.* If two nodes $N_1$ and $N_2$ hold the same set of claims
$C$ after L2 convergence, they generate identical auto-edge sets.

_Proof._ (a) L2 convergence guarantees $C_1 = C_2$. (b) The filter
(embedding $!= "None"$) is a pure predicate on claim content. (c) The
sort is by claim ID (BLAKE3 hash of canonical form), deterministic.
(d) The pair enumeration $(i, j)$ with $i < j$ is a deterministic
function of the sorted order. (e) Hamming distance is commutative
and deterministic (XOR + popcount). (f) Threshold comparison is
integer. (g) The $k$-cap tracks per-claim counts in a HashMap keyed
by claim ID, incremented in sorted-pair order. Therefore the edge
set is a pure function of $C$ and the protocol constants. #h(1em) $square$

== Bounded Edge Density

Without capping, $N$ claims produce up to $N(N-1)/2$ edges. With
`max_k_nearest = 10`, each claim generates at most 10 edges, bounding
the graph at $O(N dot k)$ edges. The `AutoEdgeGenerator` enforces this
by tracking per-claim edge counts and skipping pairs where either
claim has reached its cap.

For trust propagation (which is $O(E)$ per iteration), this bound
is critical: 10,000 claims with $k = 10$ produce at most 100,000
edges, compared to 50,000,000 without capping.

= Integration with L3

== Pipeline Order

At each epoch boundary, the complete L3 pipeline executes:

+ Collect all claims in the epoch.
+ *NEW*: Generate auto-edges via `AutoEdgeGenerator`.
+ Build `KnowledgeGraph` from all edges (manual + auto).
+ Run two-pass trust propagation (v0.2.0).
+ Run grid-aligned reduction with correlation discounting (v0.3.0).

Auto-edges are `RawEpistemicEdge` like any manual edge. They enter
the knowledge graph through the same `build_from_claims()` path and
participate in trust propagation identically.

== Orthogonality with CorrelationCell

`CorrelationCell` (v0.3.0) and `QuantizedEmbedding` (v0.4.0) serve
different purposes:

#table(
  columns: (auto, auto, auto),
  inset: 6pt,
  stroke: 0.5pt,
  [*Mechanism*], [*Question*], [*Pipeline Stage*],
  [CorrelationCell], ["Are these from the same environment?"], [Aggregation (discount)],
  [QuantizedEmbedding], ["Are these about the same topic?"], [Edge generation],
  [SemanticFingerprint], ["Are these the same measurement?"], [Reduction (compaction)],
)

A claim can carry all three: a fingerprint for grouping, a cell for
discounting, and an embedding for edge generation. They are independent
axes of the epistemic pipeline.

== Backward Compatibility

Claims with `embedding: None` produce no auto-edges. They participate
in the knowledge graph only through manual edges. All existing v0.2.0
and v0.3.0 behavior is preserved: the `AutoEdgeGenerator` is additive,
not replacing.

= Security Analysis

== Attack 1: Adversarial Embedding Manipulation

*Attack.* A Byzantine node crafts claim text whose SimHash is
artificially close to a target claim, generating fake Supports edges.

*Defense (Pre-image Resistance).* SimHash is a one-way projection: given
a target 256-bit hash, finding text that produces a nearby hash requires
brute-force search over the text space. Each attempt requires running
the canonical embedding model ($tilde$50 ms on CPU) and computing
the SimHash ($tilde$1 µs). Finding a text that flips a specific bit
requires $tilde 2^8 = 256$ attempts on average (birthday bound on a
single bit). Flipping 30 specific bits to cross the support threshold
requires coordinated manipulation --- economically expensive compared
to the reputation cost of the fake claim (delegation spending bounds
Sybil creation to $2 times$ delegator reputation).

== Attack 2: Threshold Gaming

*Attack.* Attacker crafts claims at exactly $T_"support" - 1$ to
steal Supports edges from high-reputation claims.

*Defense (Dead Zone + Strength Gradient).* The dead zone (31--199 bits)
is deliberately wide. A claim at distance 29 receives only $"strength"
= 10000 - 29 times 300 = 1300$ bps --- far weaker than a genuine
semantic match at distance 5 ($"strength" = 8500$ bps). The
trust propagation's Markovian flow divides trust proportionally by
strength, so weak auto-edges have minimal influence. Combined with
reputation weighting (0-bps Sybils contribute zero), the attack yields
negligible epistemic gain.

== Attack 3: $O(N^2)$ Computational DoS

*Attack.* Attacker floods 10,000 claims to force 50 million Hamming
distance computations.

*Defense (Hardware Speed + Rate Limiting).* Hamming distance on 256-bit
vectors is $tilde 1$ ns (hardware popcount). 50 million comparisons
$= 50$ ms --- annoying but not fatal. L2 rate limiting (50 msg/sec
per peer) bounds claim inflow. The `max_k_nearest` cap ensures the
resulting graph has at most $10000 times 10 = 100000$ edges, well
within trust propagation's capacity.

== Attack 4: Embedding Collision

*Attack.* Two semantically different claims produce identical SimHash
(all 256 bits match).

*Defense (Statistical Improbability + Layered Verification).* For
random inputs, collision probability is $2^(-256)$. For adversarial
inputs, a SimHash collision means identical projections across 256
random hyperplanes --- the texts must be nearly semantically identical
in the canonical model's latent space. Even if a collision occurs,
the trust propagation weighs the resulting edge by both nodes'
reputation and applies contradiction damping from opposing evidence.

= Evaluation

== Test Suite (17 new tests)

#table(
  columns: (auto, auto),
  inset: 5pt,
  stroke: 0.5pt,
  [*Test*], [*Verifies*],
  [`test_hamming_distance_identical`], [Distance = 0 for same embedding],
  [`test_hamming_distance_opposite`], [Distance = 256 for bitwise NOT],
  [`test_hamming_distance_single_bit`], [Distance = 1 for single flip],
  [`test_hamming_distance_symmetric`], [$d(A,B) = d(B,A)$],
  [`test_hamming_distance_known_flips`], [Exact distance for N flipped bits],
  [`test_hamming_distance_range`], [Random embeddings $approx$ 128 apart],
  [`test_auto_edge_supports`], [Close pair $arrow.r$ Supports],
  [`test_auto_edge_contradicts`], [Far pair $arrow.r$ Contradicts],
  [`test_auto_edge_dead_zone`], [Medium distance $arrow.r$ no edge],
  [`test_auto_edge_deterministic`], [Same claims any order $arrow.r$ same edges],
  [`test_auto_edge_no_embedding_skipped`], [None embedding $arrow.r$ skip],
  [`test_auto_edge_max_k_cap`], [Edge count bounded per claim],
  [`test_auto_edge_strength_gradient`], [Closer = stronger],
  [`test_auto_edge_empty_input`], [Empty $arrow.r$ no edges],
  [`test_auto_edge_single_claim`], [Single $arrow.r$ no edges],
  [`test_embedding_version_mismatch`], [Different versions $arrow.r$ no edges],
  [`test_embedding_version_same`], [Same version $arrow.r$ edges],
)

All 81 tests pass (64 existing + 17 new).

== Computational Cost

#table(
  columns: (auto, auto, auto),
  inset: 5pt,
  stroke: 0.5pt,
  [*Operation*], [*Claims*], [*Estimated Time*],
  [Hamming distance (single pair)], [2], [$tilde 1$ ns],
  [Epoch batch (all pairs)], [100], [$tilde 5$ µs],
  [Epoch batch], [1,000], [$tilde 0.5$ ms],
  [Epoch batch], [10,000], [$tilde 50$ ms],
  [Epoch batch], [100,000], [$tilde 5$ s (LSH needed)],
)

For $N <= 10000$ (typical epoch size), the batch cost is dominated
by the $O(N^2 / 2)$ pair enumeration. At $tilde 1$ ns per Hamming
distance, this is acceptable. For $N > 10000$, LSH bucketing (group
by SimHash prefix) would reduce to $O(N dot "bucket"_"size")$ ---
deferred to v0.4.1.

== Edge Density Analysis

With default thresholds ($T_"support" = 30$, $T_"contradict" = 200$)
and random embeddings (expected Hamming distance $approx 128$):

- *Supports probability*: $P(d <= 30) approx 10^(-12)$ (negligible
  for random pairs --- only semantically similar claims trigger).
- *Contradicts probability*: $P(d >= 200) approx 10^(-12)$ (symmetric).
- *Expected auto-edges per epoch*: proportional to the number of
  genuinely related claim pairs, not to $N^2$.

The dead zone captures $> 99.99%$ of random pairs, ensuring the
auto-generated graph is sparse and meaningful.

= Limitations and Future Work

+ *Canonical model dependency.* The quality of auto-generated edges
  depends on the canonical embedding model. A poor model produces
  poor SimHash approximations. Model selection requires empirical
  evaluation outside the protocol's scope.

+ *Float boundary at application side.* The SimHash construction
  requires floating-point dot products (embedding $times$ hyperplane).
  This occurs agent-side, not protocol-side. If an agent uses
  non-deterministic float arithmetic, its SimHash may differ from
  other agents'. The defense is reputation: inconsistent embeddings
  produce inconsistent edges, which are penalized by the trust
  propagation's contradiction damping.

+ *Dead zone tuning.* The default thresholds (30 / 200) are chosen
  for general-purpose text similarity. Domain-specific applications
  (e.g., sensor readings with known noise profiles) may require
  different thresholds.

+ *No cross-version edges.* Claims with different `embedding_version`
  never produce auto-edges. During model transitions, the knowledge
  graph temporarily partitions. A bridge mechanism (cross-version
  similarity via content hash) is left for future work.

+ *LSH for large epochs.* For $N > 10000$ claims per epoch, the
  $O(N^2)$ batch becomes expensive. LSH bucketing #cite(label("indyk1998"))
  would reduce this to $O(N dot "bucket")$ at the cost of missing some
  cross-bucket pairs.

= Conclusion

We presented Deterministic Semantic Topologies for the AIMP Epistemic
Layer, transforming the knowledge graph from a manually-constructed
artifact into an autonomously-assembled structure. The core mechanism
--- 256-bit SimHash with Hamming distance --- is simple, fast ($tilde 1$
ns per comparison), deterministic, and ZK-circuit-compatible.

The significance is architectural: with v0.4.0, thousands of
autonomous agents can emit claims into a peer-to-peer CRDT network,
and the protocol _automatically discovers_ which claims support or
contradict each other. Combined with the existing L3 machinery ---
reputation-weighted trust propagation (v0.2.0), correlation-aware
discounting (v0.3.0), and the Merkle-DAG CRDT transport (v0.1.0) ---
this creates a fully autonomous _Truth Discovery Engine_ that
operates without coordination, without floats, and without trust.

#bibliography("references.yml", style: "ieee")
