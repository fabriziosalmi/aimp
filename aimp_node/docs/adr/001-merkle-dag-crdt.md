# ADR 001: Merkle-DAG based CRDT for State Synchronization

## Status
Accepted

## Context
AIMP requires a deterministic, decentralized synchronization mechanism that can handle concurrent updates from multiple nodes without a central coordinator. Traditional consensus (Raft/Paxos) is too heavy for high-frequency AI inference metadata, and simple eventual consistency lacks the "causal tracking" needed for AI decision lineage.

## Decision
We chose a **Merkle-DAG based CRDT**. 
- **Merkle Tree**: Provides cryptographic integrity. Each node's hash depends on its parents and the payload.
- **DAG (Directed Acyclic Graph)**: Encodes causality. A node points to all current "heads" as its parents.
- **CRDT Properties**: The "join" (merge) operation is commutative, associative, and idempotent, verified via property-based testing.

## Consequences
- **Pros**: Zero-conflict merges, cryptographic verification of history, no single point of failure.
- **Cons**: Memory usage grows with history (addressed by ADR 002: Epoch GC).
