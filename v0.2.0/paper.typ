#set document(
  title: "Byzantine-Tolerant Belief Aggregation over Merkle-DAGs",
  author: "Fabrizio Salmi",
)
#set page(margin: (x: 2.5cm, y: 2.5cm), numbering: "1")
#set text(font: "New Computer Modern", size: 10pt)
#set par(justify: true, leading: 0.65em)
#set heading(numbering: "1.")

#align(center)[
  #text(size: 16pt, weight: "bold")[
    Byzantine-Tolerant Belief Aggregation over Merkle-DAGs:\
    A Deterministic Epistemic Layer for Decentralized AI
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
  *Abstract.* Conflict-free Replicated Data Types (CRDTs) guarantee that distributed
  nodes converge on the same data, but they do not guarantee that the data is _true_.
  In multi-agent systems where autonomous agents produce observations, inferences,
  and action intents, raw set-union convergence is insufficient: the system must also
  reason about _belief_, _trust_, and _contradiction_.

  We present an Epistemic Layer (L3) built above AIMP, a high-performance
  Merkle-CRDT protocol achieving 1.28M mutations/sec with Ed25519 signing.
  Our contributions are: (1) deterministic Bayesian aggregation via integer
  log-odds arithmetic, eliminating IEEE 754 non-reproducibility across architectures;
  (2) two-pass trust propagation over acyclic subgraphs with dynamic decay and
  configurable contradiction damping, provably convergent with spectral radius
  $rho(A) = 0$; (3) Sybil-resistant reputation via cryptographic Web of Trust
  delegation; and (4) materialized compaction that preserves epistemic graph
  integrity across asynchronous garbage collection epochs without breaking
  CRDT availability guarantees.

  The design was produced through 6 rounds of adversarial multi-AI review,
  yielding 36 unit tests and patches for 13 distinct vulnerability classes
  including sign-inversion exploits, type confusion attacks, and echo chamber
  amplification. All arithmetic is fixed-point integer; all operations are
  deterministic; all components are pluggable traits. The implementation is
  open-source (Rust, MIT license).
]

= Introduction

Conflict-free Replicated Data Types (CRDTs) #cite(label("shapiro2011")) solve the
fundamental problem of replicated state convergence in partition-tolerant
networks. By ensuring that concurrent updates commute, CRDTs guarantee
_eventual consistency_ without coordination. This property makes them
attractive for multi-agent systems where autonomous agents operate under
intermittent connectivity.

However, CRDTs are _semantically blind_: they ensure all nodes see the same
set of facts, but provide no mechanism to assess whether those facts are
_true_, _trustworthy_, or _contradictory_. When agents produce observations
from noisy sensors, inferences from local models, and intents to act in the
physical world, the system must reason about epistemic quality --- not merely
data presence.

We address this gap with an Epistemic Layer (L3) that sits above a
high-performance Merkle-CRDT transport layer (AIMP v0.1.0 #cite(label("salmi2026aimp"))).
L3 interprets opaque CRDT payloads as typed _knowledge claims_ with
provenance, confidence, and causal structure. It classifies claims into
_accepted_, _rejected_, or _uncertain_ beliefs through reputation-weighted
Bayesian aggregation --- all in deterministic integer arithmetic suitable
for Byzantine Fault Tolerant consensus.

The key insight is _architectural orthogonality_: L3 never blocks L2. The
CRDT merges unconditionally at full speed (1.28M ops/sec); the epistemic
layer processes the merged state asynchronously. This preserves the
availability guarantees of the AP data plane while adding a convergent
belief plane on top.

== Contributions

+ *Log-Odds Bayesian Aggregation.* We replace floating-point confidence
  with integer log-odds ($i$32, milli-log-odds scale). Bayesian update
  becomes addition; aggregation of independent evidence becomes summation.
  Zero underflow, zero architecture-dependent rounding, 100% deterministic.

+ *Two-Pass Trust Propagation.* We separate positive propagation
  (Supports/DerivedFrom edges) from contradiction subtraction. Pass 1
  converges on the acyclic subgraph (cyclic edges zeroed via DFS).
  Pass 2 applies contradictions in a single $O(E)$ sweep using
  stabilized values. No oscillation. Provably convergent.

+ *Sybil-Resistant Reputation.* New nodes start with reputation 0.
  Voting weight requires cryptographic delegation from an established
  anchor node (Web of Trust). Delegated reputation is capped by the
  delegator's own score. Equivocation-slashed nodes cannot delegate.

+ *Materialized Compaction.* Before epoch-based garbage collection
  prunes old DAG nodes, L3 re-injects Summary claims into L2 as fresh
  mutations. Summaries carry aggregated log-odds, variance, and range ---
  preserving epistemic graph integrity across asynchronous GC.

+ *Adversarial Multi-AI Review.* The design underwent 6 rounds of
  review by three AI systems (Claude, Gemini, ChatGPT) and a human
  architect, yielding patches for 13 vulnerability classes.

= Background and Related Work

== CRDTs and Eventual Consistency

CRDTs, formalized by Shapiro et al. #cite(label("shapiro2011")), provide
mathematically guaranteed convergence through commutative, associative,
and idempotent merge operations. Merkle-DAG CRDTs, explored by Kleppmann
#cite(label("kleppmann2022")), extend this with cryptographic integrity: each
node in the DAG is content-addressed, enabling efficient delta-sync and
tamper detection.

AIMP v0.1.0 #cite(label("salmi2026aimp")) implements a Merkle-DAG CRDT with
Ed25519 per-mutation signing, Noise Protocol XX transport, and BFT quorum
consensus. It achieves 1.28M mutations/sec in batch mode and sub-millisecond
convergence. The present work builds upon AIMP's L1/L2 stack.

== Byzantine Fault Tolerance in Distributed AI

Castro and Liskov's PBFT #cite(label("castro1999")) established practical
Byzantine consensus. Tendermint #cite(label("buchman2016")) and Casper
#cite(label("buterin2017")) introduced _equivocation slashing_ --- cryptographic
proof that a node signed contradictory messages. AIMP v0.1.0 adapts this
pattern to CRDTs, expelling Byzantine nodes while preserving unconditional
set-union merge.

The present work extends BFT from the data plane to the _belief plane_:
not only detecting Byzantine data injection, but limiting its epistemic
influence through reputation-weighted trust propagation.

== Belief Aggregation and Trust Networks

Bayesian belief aggregation in multi-agent systems has been studied
extensively #cite(label("genest1986")). The challenge in decentralized settings
is _floating-point non-determinism_: IEEE 754 arithmetic produces
architecture-dependent rounding, breaking consensus. We solve this with
log-odds integer arithmetic, following the insight that Bayesian update
in log-odds space reduces to addition #cite(label("jaynes2003")).

Trust propagation on graphs relates to PageRank #cite(label("page1999")) and
Markov Random Fields. Our two-pass approach avoids the oscillation
problems of signed-weight relaxation by separating positive and negative
influence.

= System Architecture

The AIMP stack is organized in three layers:

#figure(
  table(
    columns: (auto, auto, auto),
    inset: 8pt,
    align: left,
    [*Layer*], [*Responsibility*], [*Crate*],
    [L3: Cognitive], [Belief, trust, contradiction, compaction], [`aimp-cognitive`],
    [L2: State], [CRDT merge, epoch GC, delta-sync], [`aimp-core`],
    [L1: Transport], [Noise XX, gossip, Ed25519 identity], [`aimp-core`],
  ),
  caption: [AIMP layered architecture.],
)

*Invariant:* L3 never blocks L2. The CRDT merges at full speed regardless
of cognitive processing. L3 deserializes opaque L2 payloads on the consumer
side only.

= Log-Odds Bayesian Aggregation

== Motivation

Standard probability representation ($p in [0, 1]$) requires multiplication
for Bayesian update: $P("posterior") = P("prior") times P("likelihood") / P("evidence")$.
Recursive multiplication of fixed-point integers causes _underflow to zero_
--- a vulnerability we identified in Round 2 of our review process.

== Log-Odds Representation

We represent confidence as _log-odds_ scaled by 1000 (milli-log-odds),
stored as `i32`:

$ "logodds"(p) = 1000 times ln(p / (1 - p)) $

#figure(
  table(
    columns: (auto, auto),
    inset: 6pt,
    [*Log-Odds (i32)*], [*Probability*],
    [-6907], [\~0.1%],
    [-2197], [\~10%],
    [0], [50%],
    [+2197], [\~90%],
    [+6907], [\~99.9%],
  ),
  caption: [Log-odds integer mapping to probability.],
)

*Bayesian update becomes addition:*
$ "posterior" = "prior" + "evidence" $

*Aggregation of $N$ independent sources:*
$ "combined" = sum_(i=1)^N "evidence"_i $

All operations are saturating `i32` addition. No multiplication chains,
no underflow, no architecture-dependent rounding. Conversion to/from
probability uses a deterministic lookup table (no `ln`/`exp`).

== Echo Chamber Protection

When $N$ nodes relay the same sensor reading, naive aggregation would
count it $N$ times. Each claim carries an `evidence_source` field ---
the BLAKE3 hash of the _original_ data source. The aggregator counts
only _unique_ evidence sources.

*Limitation:* This prevents _network-level_ Sybil amplification (same
data relayed by $N$ nodes). It does not detect _correlated physical
sensor failure_ (two broken sensors reading the same wrong value). The
latter is an application-level concern requiring domain-specific
calibration.

= Trust Propagation

== Knowledge Graph

Claims are connected by typed epistemic edges:

#figure(
  table(
    columns: (auto, auto),
    inset: 6pt,
    [*Relation*], [*Semantics*],
    [Supports], [Source provides evidence for target],
    [Contradicts], [Source disputes target],
    [DerivedFrom], [Target was inferred from source],
    [SharedSource], [Same evidence origin (potential echo chamber)],
  ),
  caption: [Epistemic edge relations and semantics.],
)

== Two-Pass Algorithm

Mixing positive (Supports) and negative (Contradicts) edges in iterative
relaxation causes oscillation on non-bipartite graphs. We separate them:

*Pass 1 (Positive Propagation).* Iterate over Supports and DerivedFrom
edges only. Cyclic edges identified via DFS back-edge detection are
excluded (weight = 0). All remaining weights are non-negative on an
acyclic subgraph. Convergence is guaranteed.

For each edge $e$ from source claim $i$ (authored by node $A$):

$ "contribution" = max(0, "trust"_i) times frac("strength"_e times "reputation"_A, 10000^2) $

The $max(0, dot)$ clamp ensures rejected claims (negative trust) have
zero epistemic authority, preventing the _"enemy of my enemy"_ sign-inversion
exploit identified in Round 5. The use of $"reputation"_A$ (the author's
network-level reputation) rather than $"trust"_i$ (the claim's belief score)
prevents the Type Confusion vulnerability identified in Round 6.

*Pass 2 (Contradiction Subtraction).* Single $O(E)$ pass using stabilized
trust from Pass 1. For each Contradicts edge:

$ "penalty" = max(0, "trust"_"source") times frac("strength"_e times "reputation"_A, 10000^2) $

$ "capped_penalty" = min("penalty", alpha times max(0, "trust"_"target")) $

where $alpha in [0, 1]$ is the configurable _Contradiction Damping_ factor
(default: 0.5). The inner $max(0, dot)$ ensures the cap is never negative,
preventing sign-arithmetic anomalies when the target already has negative trust.
This limits a single high-trust node from annihilating another's trust in
one operation.

#block(fill: luma(240), inset: 10pt, radius: 4pt)[
  *Theorem 1 (Convergence).* After DFS back-edge zeroing, the positive
  sub-adjacency matrix $A$ is strictly upper triangular with respect to the
  DFS topological ordering. All eigenvalues of $A$ are 0, hence $rho(A) = 0 < 1$.
  The iterative relaxation $arrow(t)_(k+1) = arrow(t)_0 + A arrow(t)_k$ converges
  in at most $D$ steps, where $D$ is the maximum depth of the acyclic subgraph.
  Pass 2 is a single $O(E)$ sweep, requiring no iteration.

  *Corollary.* The Belief Engine produces identical results on all nodes
  regardless of message arrival order, satisfying BFT determinism.
]

== Dynamic Decay

Trust decay per edge is not a protocol constant but the _effective weight_
of the edge: $"strength"_e times "reputation"_"author"$. An edge with strength
9500 (95%) barely attenuates; an edge with strength 1000 (10%) heavily
attenuates. The network determines its own trust topology.

= Sybil Defense

== The Attack

An adversary generates $K$ fresh Ed25519 keys. If default reputation is
$R_0 > 0$, total fake voting weight is $K times R_0$ --- unbounded.

== Web of Trust

New nodes start with reputation 0. To gain voting weight, a node must be
_delegated_ by an existing trusted node:

$ "delegated_rep" = min("requested_rep", "delegator_rep") $

Delegation rules:
- Capped by delegator's own reputation (cannot grant more than you have)
- Equivocation-slashed nodes (reputation = 0) cannot delegate
- Creates a cryptographic trust chain rooted in anchor nodes

For the present work, we adopt a permissioned model (Option A: explicit
delegation). Permissionless reputation bootstrapping through prediction
accuracy is left to future work.

= Materialized Compaction

== The Split-Brain Problem

L2 epoch GC deletes old DAG nodes. If L3's Knowledge Graph has edges
pointing to deleted nodes, it encounters dangling references.

== Solution

Before epoch GC triggers, L3's SemanticReducer produces Summary claims
and re-injects them into L2 as _new mutations_. The Summary becomes
ground truth in the current epoch; old nodes can safely be pruned.

Summaries carry:
- `aggregated_logodds`: combined confidence (log-odds sum of unique sources)
- `variance_milli`: integer variance of input confidences (detects disagreement)
- `range_min`, `range_max`: full confidence spread (preserves outlier information)
- `unique_sources`: count of independent evidence origins
- `evidence_source`: Summary's own BLAKE3 hash (unique, prevents dedup collision)

The SemanticReducer is required to be _commutative_, _associative_, and
_idempotent_ --- the same mathematical properties that make CRDTs converge.
This ensures that Summaries produced on different nodes from different
DAG states are merge-compatible.

= Belief Engine

The `LogOddsBeliefEngine` classifies claims into three categories:

#figure(
  table(
    columns: (auto, auto, auto),
    inset: 6pt,
    [*Category*], [*Condition*], [*Meaning*],
    [Accepted], [final log-odds $>=$ 2197], [Believed true (\~90%)],
    [Rejected], [final log-odds $<=$ -2197], [Believed false (\~10%)],
    [Uncertain], [between thresholds], [Insufficient evidence],
  ),
  caption: [Belief Engine classification thresholds.],
)

The pipeline:

+ Compute base trust: $"reputation"_("author"(i)) times "confidence"_i$ for each claim
+ Pass 1: propagate positive trust through acyclic subgraph
+ Pass 2: subtract contradictions (capped, single pass)
+ Classify each claim against thresholds

= Security Analysis

Six rounds of adversarial review identified and patched 13 vulnerability
classes:

#figure(
  table(
    columns: (auto, auto, auto),
    inset: 6pt,
    align: left,
    [*Vulnerability*], [*Round*], [*Fix*],
    [IEEE 754 non-determinism], [R1], [Log-odds i32 arithmetic],
    [u16 Bayesian underflow], [R2], [Log-odds eliminates multiplication],
    [L2/L3 GC split-brain], [R2], [Materialized compaction],
    [Reducer non-convergence], [R2], [Commutative/associative/idempotent requirement],
    [Cyclic confidence inflation], [R3], [DFS back-edge zeroing],
    [Trust oscillation], [R3], [Two-pass separation],
    [Hardcoded decay], [R3], [Dynamic edge strength],
    [Confidence gaming], [R4], [Reputation-gated edge weight],
    [Single-actor annihilation], [R4], [Configurable damping cap],
    [Sign-inversion exploit], [R5], [Trust clamping max(0)],
    [Sybil amplification], [R5], [Zero default rep + Web of Trust],
    [Type confusion (logodds as rep)], [R6], [Separate reputation lookup],
    [Summary evidence collision], [R6], [Unique evidence_source per Summary],
  ),
  caption: [Vulnerability classes patched during multi-AI review.],
)

= Evaluation

== Test Coverage

The prototype includes 36 unit tests covering:

#figure(
  table(
    columns: (auto, auto),
    inset: 6pt,
    [*Category*], [*Tests*],
    [Log-odds arithmetic], [4],
    [Reputation weighting], [2],
    [Echo chamber protection], [2],
    [Reducer invariants (commutativity, idempotency)], [2],
    [Knowledge graph traversal], [2],
    [Belief classification], [2],
    [Intent resolution], [1],
    [Cycle detection and edge zeroing], [4],
    [Trust propagation (two-pass, dynamic decay)], [5],
    [Summary variance and range], [3],
    [Sign-inversion exploit], [1],
    [Sybil defense and delegation], [4],
  ),
  caption: [Unit test coverage by category.],
)

All tests use deterministic integer arithmetic and produce identical
results on ARM64 and x86_64.

== Design Validation Methodology

Rather than empirical simulation (which would require a multi-agent
testbed beyond the scope of this report), we validated the design through
_adversarial multi-AI review_: six rounds of analysis by three AI systems
(Claude, Gemini, ChatGPT) and a human architect, each tasked with finding
exploitable vulnerabilities. This process identified 13 distinct bug
classes, all patched and tested. We believe this methodology is
complementary to formal verification and may be of independent interest.

= Limitations and Future Work

+ *Network-level only.* Echo chamber protection detects Sybil amplification
  (same data relayed by $N$ nodes) but not correlated physical sensor failure.

+ *Permissioned trust.* The current Web of Trust requires explicit delegation.
  Permissionless reputation bootstrapping (e.g., prediction markets or
  proof-of-accuracy) is future work.

+ *No empirical multi-agent evaluation.* The present work validates
  correctness via unit tests and formal reasoning. Large-scale simulation
  with heterogeneous agents under adversarial conditions is planned.

+ *Reducer limitations.* The ExactMatchReducer groups by semantic fingerprint
  equality. Threshold-based and embedding-based grouping (with quantized
  integer embeddings) are future work.

+ *Reputation inflation.* A trusted delegator can grant full reputation to
  $N$ new nodes, creating a soft Sybil vector. Reputation _spending_ (where
  delegation reduces the delegator's own score) would close this gap.

+ *Formal verification.* The convergence lemma (Theorem 1) is stated but
  not machine-checked. Encoding the two-pass algorithm in TLA+ or Coq
  is planned.

= Conclusion

We presented an Epistemic Layer for Merkle-DAG CRDTs that transforms
raw data convergence into _belief convergence_. The system reasons about
trust, contradiction, and evidence quality using deterministic integer
arithmetic, making it suitable for Byzantine Fault Tolerant consensus
in decentralized multi-agent networks.

The design was forged through an unusual process: six rounds of adversarial
review by three AI systems and a human architect, each probing for
mathematical, cryptographic, and systems-level vulnerabilities. This
collaborative human-AI methodology produced a system that none of the
participants could have conceived alone with equivalent depth and rigor.

The implementation is open-source (Rust, MIT license) and available at
#link("https://github.com/fabriziosalmi/aimp").

#heading(numbering: none)[References]

#set text(size: 9pt)

#bibliography("references.yml", style: "ieee")
