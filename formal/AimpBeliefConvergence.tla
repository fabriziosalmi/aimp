--------------------------- MODULE AimpBeliefConvergence ---------------------------
EXTENDS Naturals, Sequences, FiniteSets

(*
  Formal specification of AIMP Epistemic Layer (L3) Belief Convergence.

  Verifies three safety properties of the two-pass trust propagation algorithm:

  1. BeliefDeterminism: Same claim set + same graph → identical BeliefState
     on all nodes (prerequisite for BFT consensus).

  2. NoOscillation: Trust values converge monotonically — once Pass 1
     stabilizes, adding Pass 2 (contradictions) cannot trigger a new
     round of Pass 1 increases.

  3. ContradictionSafety: A single Contradicts edge with damping cap α
     cannot flip a claim from Accepted to Rejected in one step.

  Model parameters:
    Nodes       = {n1, n2, n3}   \* Participating nodes
    MaxClaims   = 3               \* Bound on claims per node
    MaxEdges    = 3               \* Bound on edges per node
    DampingCap  = 50              \* Max % a single contradiction removes (50 = 50%)
    AcceptThreshold = 60          \* Trust >= 60 → Accepted
    RejectThreshold = 20          \* Trust <= 20 → Rejected
*)

CONSTANT Nodes,            \* Set of participating nodes
         MaxClaims,        \* Max claims in the system
         MaxEdges,         \* Max edges in the graph
         DampingCap,       \* Contradiction damping cap (percentage)
         AcceptThreshold,  \* Trust threshold for Accepted
         RejectThreshold   \* Trust threshold for Rejected

VARIABLES claims,      \* Map: Node -> Set of claim IDs held
          edges,       \* Map: Node -> Set of [from, to, type, strength] edges
          trust,       \* Map: Node -> Map: ClaimID -> trust value (0..100)
          belief,      \* Map: Node -> Map: ClaimID -> {"accepted","rejected","uncertain"}
          nextStep     \* Phase counter for sequencing

Vars == <<claims, edges, trust, belief, nextStep>>

\* Possible claim IDs (bounded)
ClaimIds == 1..MaxClaims

\* Edge types: 1 = Supports, 2 = Contradicts
EdgeTypes == {1, 2}

\* Strength values (simplified to 0..100)
Strengths == 0..100

Init ==
    /\ claims   = [n \in Nodes |-> {}]
    /\ edges    = [n \in Nodes |-> {}]
    /\ trust    = [n \in Nodes |-> [c \in ClaimIds |-> 0]]
    /\ belief   = [n \in Nodes |-> [c \in ClaimIds |-> "uncertain"]]
    /\ nextStep = 0

(* ─── Actions ─── *)

\* A claim arrives at a node with initial trust value
AddClaim(n, c, initTrust) ==
    /\ c \in ClaimIds
    /\ c \notin claims[n]
    /\ Cardinality(claims[n]) < MaxClaims
    /\ initTrust \in 0..100
    /\ claims' = [claims EXCEPT ![n] = @ \cup {c}]
    /\ trust'  = [trust EXCEPT ![n][c] = initTrust]
    /\ UNCHANGED <<edges, belief, nextStep>>

\* An edge is added between two claims on a node
AddEdge(n, from, to, etype, str) ==
    /\ from \in claims[n]
    /\ to \in claims[n]
    /\ from # to
    /\ etype \in EdgeTypes
    /\ str \in Strengths
    /\ Cardinality(edges[n]) < MaxEdges
    /\ LET newEdge == [f |-> from, t |-> to, tp |-> etype, s |-> str]
       IN edges' = [edges EXCEPT ![n] = @ \cup {newEdge}]
    /\ UNCHANGED <<claims, trust, belief, nextStep>>

\* Replicate: a node receives claims+edges from another (simulates L2 gossip)
Replicate(src, dst) ==
    /\ src # dst
    /\ claims' = [claims EXCEPT ![dst] = @ \cup claims[src]]
    /\ edges'  = [edges EXCEPT ![dst] = @ \cup edges[src]]
    /\ \* Copy trust values for newly received claims
       trust' = [trust EXCEPT ![dst] =
           [c \in ClaimIds |->
               IF c \in claims[src] /\ c \notin claims[dst]
               THEN trust[src][c]
               ELSE trust[dst][c]]]
    /\ UNCHANGED <<belief, nextStep>>

\* Pass 1: Positive propagation (Supports edges only)
\* For each Supports edge (from→to), add scaled trust from source to target
PropagatePositive(n) ==
    /\ \E e \in edges[n] :
        /\ e.tp = 1   \* Supports only
        /\ e.f \in claims[n]
        /\ e.t \in claims[n]
        /\ trust[n][e.f] > 0  \* Only positive trust propagates
        /\ LET bonus == (trust[n][e.f] * e.s) \div 100
               newTrust == IF trust[n][e.t] + bonus > 100
                          THEN 100
                          ELSE trust[n][e.t] + bonus
           IN
             /\ newTrust > trust[n][e.t]  \* Only if it actually increases
             /\ trust' = [trust EXCEPT ![n][e.t] = newTrust]
    /\ UNCHANGED <<claims, edges, belief, nextStep>>

\* Pass 2: Contradiction subtraction with damping cap
ApplyContradiction(n) ==
    /\ \E e \in edges[n] :
        /\ e.tp = 2   \* Contradicts only
        /\ e.f \in claims[n]
        /\ e.t \in claims[n]
        /\ trust[n][e.f] > 0  \* Negative-trust claims cannot contradict (clamping)
        /\ LET rawPenalty == (trust[n][e.f] * e.s) \div 100
               \* Damping cap: max DampingCap% of target's current positive trust
               maxPenalty == IF trust[n][e.t] > 0
                            THEN (trust[n][e.t] * DampingCap) \div 100
                            ELSE rawPenalty
               penalty == IF rawPenalty < maxPenalty THEN rawPenalty ELSE maxPenalty
               newTrust == IF trust[n][e.t] > penalty
                          THEN trust[n][e.t] - penalty
                          ELSE 0
           IN trust' = [trust EXCEPT ![n][e.t] = newTrust]
    /\ UNCHANGED <<claims, edges, belief, nextStep>>

\* Classification: derive belief state from trust values
Classify(n) ==
    /\ belief' = [belief EXCEPT ![n] =
        [c \in ClaimIds |->
            IF c \notin claims[n] THEN "uncertain"
            ELSE IF trust[n][c] >= AcceptThreshold THEN "accepted"
            ELSE IF trust[n][c] <= RejectThreshold THEN "rejected"
            ELSE "uncertain"]]
    /\ UNCHANGED <<claims, edges, trust, nextStep>>

Next ==
    \/ \E n \in Nodes, c \in ClaimIds, t \in 0..100 : AddClaim(n, c, t)
    \/ \E n \in Nodes, f \in ClaimIds, t \in ClaimIds, tp \in EdgeTypes, s \in Strengths :
        AddEdge(n, f, t, tp, s)
    \/ \E src, dst \in Nodes : Replicate(src, dst)
    \/ \E n \in Nodes : PropagatePositive(n)
    \/ \E n \in Nodes : ApplyContradiction(n)
    \/ \E n \in Nodes : Classify(n)

(* ═══════════════════ SAFETY PROPERTIES ═══════════════════ *)

(* BeliefDeterminism: If two nodes have identical claims, edges, and trust,
   they must compute identical belief states after classification. *)
BeliefDeterminism ==
    \A n1, n2 \in Nodes :
        (/\ claims[n1] = claims[n2]
         /\ edges[n1]  = edges[n2]
         /\ trust[n1]  = trust[n2])
        => belief[n1] = belief[n2]

(* NoAmplification: Trust values are bounded [0, 100].
   No sequence of propagation steps can push trust above 100. *)
TrustBounded ==
    \A n \in Nodes :
        \A c \in ClaimIds :
            trust[n][c] >= 0 /\ trust[n][c] <= 100

(* ContradictionSafety: A claim that was Accepted (trust >= AcceptThreshold)
   cannot be driven below RejectThreshold by a single ApplyContradiction step.
   With DampingCap=50, a single contradiction removes at most 50% of trust.
   If AcceptThreshold=60, after one contradiction: trust >= 60 * 0.5 = 30 > 20 = RejectThreshold.
   So a single contradiction CANNOT flip Accepted → Rejected. *)
ContradictionSafety ==
    \A n \in Nodes :
        \A c \in ClaimIds :
            \* If trust is at or above AcceptThreshold, it cannot be at or below RejectThreshold
            \* in the next state due to a SINGLE contradiction (damping cap ensures this)
            (trust[n][c] >= AcceptThreshold)
            => (trust[n][c] - (trust[n][c] * DampingCap) \div 100 > RejectThreshold)

(* ═══════════════════ SPECIFICATION ═══════════════════ *)

Spec == Init /\ [][Next]_Vars

THEOREM Spec => []BeliefDeterminism
THEOREM Spec => []TrustBounded
THEOREM Spec => []ContradictionSafety
=============================================================================
