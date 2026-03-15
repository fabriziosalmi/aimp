# ADR 002: Epoch-based Garbage Collection (GC) for DAG Maintenance

## Status
Accepted

## Context
The Merkle-DAG (ADR 001) grows linearly with every mutation. For long-running AI nodes, this results in unbounded memory consumption. A mechanism is needed to prune history while maintaining the integrity of the "active" frontier.

## Decision
We implemented **Epoch-based Garbage Collection**.
- **Frontier Preservation**: Always keep all "Heads".
- **History Bounding**: Keep a configurable number of levels (`DAG_HISTORY_DEPTH`) behind the frontier.
- **Determinism**: GC is triggered every `GC_MUTATION_THRESHOLD` mutations, ensuring all honest nodes with the same history perform the same pruning.

## Consequences
- **Pros**: Constant memory footprint for stable frontiers.
- **Cons**: Deep historical audits require external archival nodes (outside the core protocol scope).
