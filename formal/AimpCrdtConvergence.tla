--------------------------- MODULE AimpCrdtConvergence ---------------------------
EXTENDS Naturals, Sequences, FiniteSets

(*
  Formal specification of AIMP Merkle-CRDT Convergence.
  Goal: Prove that even with arbitrary mutation ordering and network partitions,
  honest nodes eventually converge to the same Merkle Root when they possess 
  the same set of DagNodes.
*)

CONSTANT Nodes,      \* Set of all participating nodes
         MaxMutations \* Bound for model checking

VARIABLES store,     \* Map: Node -> Set of DagNodes possessed
          heads,     \* Map: Node -> Set of Current Frontier Hashes
          network    \* Set of "in-flight" DagNodes being broadcasted

Vars == <<store, heads, network>>

(* 
  A DagNode is simplified to its ID (hash) and its set of parents.
  In this model, the hash is unique and deterministic based on parents.
*)

TypeOK ==
    /\ store \in [Nodes -> SUBSET [id: Nat, parents: SUBSET Nat]]
    /\ heads \in [Nodes -> SUBSET Nat]
    /\ network \in SUBSET [id: Nat, parents: SUBSET Nat]

Init ==
    /\ store = [n \in Nodes |-> {}]
    /\ heads = [n \in Nodes |-> {}]
    /\ network = {}

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

(* A node receives a DagNode from the network *)
Receive(n) ==
    /\ \E msg \in network:
        /\ msg \notin store[n]
        /\ store' = [store EXCEPT ![n] = @ \cup {msg}]
        /\ LET 
              \* Update heads: add new, remove parents that are now eclipsed
              newHeads == (heads[n] \cup {msg.id}) \ {p \in msg.parents : TRUE}
           IN 
              heads' = [heads EXCEPT ![n] = newHeads]
        /\ UNCHANGED network

Next ==
    \/ \E n \in Nodes : Mutate(n)
    \/ \E n \in Nodes : Receive(n)

(* Liveness: If no more mutations occur, do all nodes eventually have the same heads? *)
Convergence ==
    (\A n1, n2 \in Nodes : store[n1] = store[n2] => heads[n1] = heads[n2])

Spec == Init /\ [][Next]_Vars /\ WF_Vars(Next)

THEOREM Spec => []Convergence
=============================================================================
