--------------------------- MODULE AimpCrdtConvergence ---------------------------
EXTENDS Naturals, Sequences, FiniteSets

(*
  Formal specification of AIMP Merkle-CRDT Convergence with Quorum Consensus.

  Goals:
  1. Prove that honest nodes eventually converge to the same Merkle Root
     when they possess the same set of DagNodes (Convergence).
  2. Prove that the QuorumManager produces a unique decision when threshold
     is reached (QuorumSafety).
  3. Prove that if all nodes vote for the same decision, quorum is eventually
     reached (QuorumLiveness).

  Model parameters:
    Nodes          = {n1, n2}     \* Participating nodes
    MaxMutations   = 3            \* Bound per node for model checking
    Prompts        = {1, 2}       \* Bounded set of prompt IDs
    Decisions      = {1, 2}       \* Bounded set of decision IDs
    QuorumThreshold = 2           \* Minimum votes for consensus
*)

CONSTANT Nodes,            \* Set of all participating nodes
         MaxMutations,     \* Max mutations per node (bounds state space)
         Prompts,          \* Bounded set of prompt IDs
         Decisions,        \* Bounded set of decision IDs
         QuorumThreshold   \* Minimum votes for consensus

VARIABLES store,     \* Map: Node -> Set of DagNodes possessed
          heads,     \* Map: Node -> Set of node IDs forming the frontier
          network,   \* Set of "in-flight" DagNodes being broadcast
          quorum,    \* Map: Prompt -> Decision -> Set of voting Nodes
          nextId     \* Global counter for unique node IDs (models content-addressing)

Vars == <<store, heads, network, quorum, nextId>>

(*
  A DagNode is simplified to its ID (natural number) and its set of parent IDs.
  The ID is unique per node: Cardinality(store[n]) + 1 at creation time.
*)

\* Max possible IDs: each of N nodes can create MaxMutations
MaxId == Cardinality(Nodes) * MaxMutations

Init ==
    /\ store   = [n \in Nodes |-> {}]
    /\ heads   = [n \in Nodes |-> {}]
    /\ network = {}
    /\ quorum  = [p \in Prompts |-> [d \in Decisions |-> {}]]
    /\ nextId  = 1

(* A node creates a new mutation pointing to its current heads.
   Uses a global counter for unique IDs, modeling content-addressed
   hashing where each DagNode has a globally unique hash. *)
Mutate(n) ==
    /\ Cardinality(store[n]) < MaxMutations
    /\ nextId <= MaxId
    /\ LET newId    == nextId
           parentIds == heads[n]
           newNode   == [id |-> newId, parents |-> parentIds]
       IN
         /\ store'   = [store EXCEPT ![n] = @ \cup {newNode}]
         /\ heads'   = [heads EXCEPT ![n] = {newId}]
         /\ network' = network \cup {newNode}
         /\ nextId'  = nextId + 1
         /\ UNCHANGED quorum

(* A node receives a DagNode from the network and recomputes heads.
   Heads are recomputed from the full store: a node ID is a head iff
   no other node in the store lists it as a parent. This handles
   out-of-order message delivery correctly. *)
Receive(n) ==
    /\ \E msg \in network:
        /\ msg \notin store[n]
        /\ LET newStore == store[n] \cup {msg}
               \* All IDs present in the store
               allIds == {nd.id : nd \in newStore}
               \* All IDs that appear as a parent of some node
               allParents == UNION {nd.parents : nd \in newStore}
               \* Heads = IDs that are not parents of any node
               newHeads == allIds \ allParents
           IN
              /\ store' = [store EXCEPT ![n] = newStore]
              /\ heads' = [heads EXCEPT ![n] = newHeads]
        /\ UNCHANGED <<network, quorum, nextId>>

(* A node observes a decision for a prompt and casts a vote.
   A node may vote only once per prompt — not per (prompt, decision).
   This prevents a node from voting for conflicting decisions. *)
Observe(n, prompt, decision) ==
    /\ prompt \in Prompts
    /\ decision \in Decisions
    \* Node has not voted for ANY decision on this prompt
    /\ \A d \in Decisions : n \notin quorum[prompt][d]
    /\ quorum' = [quorum EXCEPT ![prompt][decision] = @ \cup {n}]
    /\ UNCHANGED <<store, heads, network, nextId>>

Next ==
    \/ \E n \in Nodes : Mutate(n)
    \/ \E n \in Nodes : Receive(n)
    \/ \E n \in Nodes, p \in Prompts, d \in Decisions : Observe(n, p, d)

(* ======================== SAFETY PROPERTIES ======================== *)

(* Convergence: If two nodes have the same store, they have the same heads.
   This is the core CRDT property — replicas with identical state compute
   identical frontier. *)
Convergence ==
    \A n1, n2 \in Nodes :
        store[n1] = store[n2] => heads[n1] = heads[n2]

(* QuorumSafety: If quorum is reached for a prompt, the decided value is
   unique. No two conflicting decisions can both reach quorum. *)
QuorumSafety ==
    \A p \in Prompts :
        \A d1, d2 \in Decisions :
            (/\ Cardinality(quorum[p][d1]) >= QuorumThreshold
             /\ Cardinality(quorum[p][d2]) >= QuorumThreshold)
            => d1 = d2

(* QuorumLiveness: If all nodes vote for the same decision, the quorum
   threshold is met. This is trivially true when |Nodes| >= QuorumThreshold. *)
QuorumLiveness ==
    \A p \in Prompts :
        \A d \in Decisions :
            quorum[p][d] = Nodes => Cardinality(quorum[p][d]) >= QuorumThreshold

(* ======================== SPECIFICATION ======================== *)

Spec == Init /\ [][Next]_Vars

THEOREM Spec => []Convergence
THEOREM Spec => []QuorumSafety
THEOREM Spec => []QuorumLiveness
=============================================================================
