# Demo 8 — BFT Quorum Consensus

**Format:** Python simulation (stdlib only)  
**Use case:** Demonstrates Byzantine Fault Tolerant quorum voting — how many nodes must agree, what happens with faulty nodes, and how the system handles network partitions.

## What it shows

- A cluster of N nodes each cast a **signed vote** on a proposed value.
- The quorum manager collects votes and commits when ≥ threshold agree.
- **Byzantine node**: one node sends conflicting votes — the quorum detects the equivocation.
- **Network partition**: a minority partition cannot commit; the majority does.
- Results include per-round vote tallies and commit/reject decisions.

## How to run

```bash
# Default: 5 nodes, threshold=3
python3 simulate.py

# Custom cluster size
python3 simulate.py --nodes 7 --threshold 5 --byzantine 1
```

No external dependencies — Python 3.8+ standard library only.

## Expected output

```
=== AIMP BFT Quorum Consensus Demo ===
Cluster: 5 nodes  threshold=3  byzantine=1

Round 1 — Proposal: "value=42"
  node-0  votes YES  (sig=a1b2c3d4...)
  node-1  votes YES  (sig=b2c3d4e5...)
  node-2  votes YES  (sig=c3d4e5f6...)
  node-3  votes YES  (sig=d4e5f6a7...)
  node-4* votes YES  (Byzantine — also voting NO!)  ⚠ equivocation detected
  Quorum: 4/5 honest YES → ✅ COMMITTED: value=42

Round 2 — Partition: nodes [3,4] isolated
  node-0  votes YES
  node-1  votes YES
  node-2  votes YES
  [node-3 isolated]  [node-4 isolated]
  Quorum (majority): 3/3 → ✅ COMMITTED
  Quorum (minority): 0/2 → ❌ CANNOT COMMIT (below threshold)
```
