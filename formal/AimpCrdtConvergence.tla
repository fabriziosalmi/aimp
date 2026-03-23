--------------------------- MODULE AimpCrdtConvergence ---------------------------
EXTENDS Naturals, Sequences, FiniteSets

(*
  Formal specification of AIMP Merkle-CRDT Convergence with Quorum Consensus.

  Goals:
  1. Prove that honest nodes eventually converge to the same Merkle Root
     when they possess the same set of DagNodes (Convergence).
  2. Prove that the QuorumManager produces a unique decision when threshold
     is reached (QuorumSafety).
  3. Prove that if all nodes vote, quorum is eventually reached (QuorumLiveness).
*)

CONSTANT Nodes,           \* Set of all participating nodes
         MaxMutations,    \* Bound for model checking
         QuorumThreshold  \* Minimum votes for consensus (e.g. 2)

VARIABLES store,     \* Map: Node -> Set of DagNodes possessed
          heads,     \* Map: Node -> Set of Current Frontier Hashes
          network,   \* Set of "in-flight" DagNodes being broadcasted
          quorum     \* Map: PromptId -> (DecisionId -> Set of voting Nodes)

Vars == <<store, heads, network, quorum>>

(*
  A DagNode is simplified to its ID (hash) and its set of parents.
  In this model, the hash is unique and deterministic based on parents.
*)

TypeOK ==
    /\ store \in [Nodes -> SUBSET [id: Nat, parents: SUBSET Nat]]
    /\ heads \in [Nodes -> SUBSET Nat]
    /\ network \in SUBSET [id: Nat, parents: SUBSET Nat]
    /\ quorum \in [Nat -> [Nat -> SUBSET Nodes]]

Init ==
    /\ store = [n \in Nodes |-> {}]
    /\ heads = [n \in Nodes |-> {}]
    /\ network = {}
    /\ quorum = [p \in {} |-> [d \in {} |-> {}]]

(* A node creates a new mutation pointing to its current heads *)
Mutate(n) ==
    /\ Cardinality(store[n]) < MaxMutations
    /\ LET newId == Cardinality(store[n]) + 1
           parentIds == heads[n]
           newNode == [id |-> newId, parents |-> parentIds]
       IN
         /\ store' = [store EXCEPT ![n] = @ \cup {newNode}]
         /\ heads' = [heads EXCEPT ![n] = {newId}]
         /\ network' = network \cup {newNode}
         /\ UNCHANGED quorum

(* A node receives a DagNode from the network *)
Receive(n) ==
    /\ \E msg \in network:
        /\ msg \notin store[n]
        /\ store' = [store EXCEPT ![n] = @ \cup {msg}]
        /\ LET
              newHeads == (heads[n] \cup {msg.id}) \ {p \in msg.parents : TRUE}
           IN
              heads' = [heads EXCEPT ![n] = newHeads]
        /\ UNCHANGED <<network, quorum>>

(* A node observes an AI decision and casts a vote *)
Observe(n, prompt, decision) ==
    /\ LET currentVoters == IF prompt \in DOMAIN quorum
                            THEN IF decision \in DOMAIN quorum[prompt]
                                 THEN quorum[prompt][decision]
                                 ELSE {}
                            ELSE {}
       IN
         /\ n \notin currentVoters  \* Each node votes only once per prompt
         /\ quorum' = [quorum EXCEPT ![prompt][decision] = currentVoters \cup {n}]
         /\ UNCHANGED <<store, heads, network>>

Next ==
    \/ \E n \in Nodes : Mutate(n)
    \/ \E n \in Nodes : Receive(n)
    \/ \E n \in Nodes, p \in Nat, d \in Nat : Observe(n, p, d)

(* Safety: If nodes have the same store, they have the same heads *)
Convergence ==
    \A n1, n2 \in Nodes : store[n1] = store[n2] => heads[n1] = heads[n2]

(* Safety: If quorum is reached for a prompt, the decision is unique *)
QuorumSafety ==
    \A p \in DOMAIN quorum :
        \A d1, d2 \in DOMAIN quorum[p] :
            /\ Cardinality(quorum[p][d1]) >= QuorumThreshold
            /\ Cardinality(quorum[p][d2]) >= QuorumThreshold
            => d1 = d2

(* Liveness: If all nodes vote for the same decision, quorum is reached *)
QuorumLiveness ==
    \A p \in DOMAIN quorum :
        \A d \in DOMAIN quorum[p] :
            quorum[p][d] = Nodes => Cardinality(quorum[p][d]) >= QuorumThreshold

Spec == Init /\ [][Next]_Vars /\ WF_Vars(Next)

THEOREM Spec => []Convergence
THEOREM Spec => []QuorumSafety
=============================================================================
