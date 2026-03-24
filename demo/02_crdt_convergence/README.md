# Demo 2 — CRDT Convergence (Interactive HTML)

**Format:** Self-contained HTML/JS (no server needed)  
**Use case:** Visualise how three AIMP nodes with a Merkle-DAG CRDT converge to a single root hash — even after a network partition.

## What it shows

- Three nodes, each with a live Merkle-DAG entry list and vector clock.
- **"Each Node Senses"** — appends a new sensor reading to each node's local log.
- **"Gossip Round"** — nodes exchange state diffs; newly merged entries are highlighted.
- **"Partition Node-C"** — node-C is isolated; A and B continue gossiping while C drifts.
- **"Heal Partition"** — C reconnects; the CRDT merge resolves all divergence automatically.
- Root hashes turn green and display ✓ when all online nodes share the same root.

## How to run

```bash
open demo/02_crdt_convergence/index.html
# or
xdg-open demo/02_crdt_convergence/index.html
# or just double-click the file in your file manager
```

No build step, no npm, no server — works in any modern browser.

## Suggested walkthrough

1. Click **Each Node Senses** 3 times, watching roots diverge.
2. Click **Gossip Round** — roots should converge (turn green).
3. Click **Partition Node-C**, then sense + gossip a few times.
4. Click **Heal Partition** — watch C automatically re-merge.
5. Or just press **▶ Auto-Run Demo** for a hands-free walkthrough.
