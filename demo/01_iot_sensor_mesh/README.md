# Demo 1 — IoT Sensor Mesh

**Format:** Python simulation (stdlib only)  
**Use case:** Multiple IoT sensors forming a peer-to-peer gossip mesh and synchronising their CRDT state.

## What it shows

- Each "node" holds a local **Merkle-DAG** CRDT (simplified as a hash-chained log).
- Nodes gossip **state diffs** to each other in rounds.
- After a configurable network partition the isolated sub-graphs re-merge and **converge** to the same root hash.
- Vector clocks track causality; duplicate messages are deduplicated.

## How to run

```bash
python3 simulate.py
```

No external dependencies — Python 3.8+ standard library only.

## Expected output

```
=== AIMP IoT Sensor Mesh Simulation ===
Round 1 — gossip...
  node-A  root=a1b2c3d4  mutations=1
  node-B  root=e5f6a7b8  mutations=1
  node-C  root=c9d0e1f2  mutations=1
...
[PARTITION] node-C isolated from mesh
Round 4 — gossip (partitioned)...
...
[HEAL] partition lifted
Round 7 — gossip (healing)...
  node-A  root=deadbeef  mutations=9  ✓ converged
  node-B  root=deadbeef  mutations=9  ✓ converged
  node-C  root=deadbeef  mutations=9  ✓ converged
```
