#!/usr/bin/env python3
"""
Demo 1 — IoT Sensor Mesh
========================
Simulates multiple AIMP nodes forming a P2P gossip mesh and synchronising
their Merkle-DAG CRDT state.  No external dependencies required.

Key concepts demonstrated:
  • Merkle-chained mutation log (simplified Merkle-DAG)
  • Vector clock causality tracking
  • Gossip-based state propagation
  • CRDT convergence after a network partition and heal
"""
import hashlib
import random
import time
from dataclasses import dataclass, field
from typing import Dict, List, Optional, Set, Tuple


# ---------------------------------------------------------------------------
# Merkle-DAG CRDT (simplified)
# ---------------------------------------------------------------------------

def _blake3_sim(data: bytes) -> str:
    """SHA-256 as a stand-in for BLAKE3 (same interface, stdlib only)."""
    return hashlib.sha256(data).hexdigest()[:8]


@dataclass
class Mutation:
    """A single CRDT mutation entry."""
    key: str
    value: str
    node_id: str
    seq: int
    parent_hash: str

    @property
    def hash(self) -> str:
        blob = f"{self.key}:{self.value}:{self.node_id}:{self.seq}:{self.parent_hash}"
        return _blake3_sim(blob.encode())


@dataclass
class VectorClock:
    """Causal vector clock."""
    clocks: Dict[str, int] = field(default_factory=dict)

    def increment(self, node_id: str) -> None:
        self.clocks[node_id] = self.clocks.get(node_id, 0) + 1

    def merge(self, other: "VectorClock") -> None:
        for nid, t in other.clocks.items():
            self.clocks[nid] = max(self.clocks.get(nid, 0), t)

    def copy(self) -> "VectorClock":
        return VectorClock(dict(self.clocks))


class MerkleLog:
    """Append-only Merkle-chained log (represents the local CRDT DAG)."""

    GENESIS = "00000000"

    def __init__(self, node_id: str) -> None:
        self.node_id = node_id
        self.entries: List[Mutation] = []
        self.seen: Set[str] = set()
        self.vclock = VectorClock()
        self._root = self.GENESIS

    # ---- mutations ----

    def append(self, key: str, value: str) -> Mutation:
        self.vclock.increment(self.node_id)
        seq = self.vclock.clocks[self.node_id]
        m = Mutation(key, value, self.node_id, seq, self._root)
        self._apply(m)
        return m

    def _apply(self, m: Mutation) -> bool:
        """Apply a mutation; return False if it was already seen."""
        if m.hash in self.seen:
            return False
        self.seen.add(m.hash)
        self.entries.append(m)
        self._recompute_root()
        return True

    def _recompute_root(self) -> None:
        """Recompute root as a deterministic hash of the sorted entry-hash set.

        Sorting by hash ensures that regardless of the order entries were
        received (different on each node after a partition), the root is
        always the same once the entry sets are equal — the CRDT guarantee.
        """
        sorted_hashes = sorted(self.seen)
        combined = "|".join(sorted_hashes)
        self._root = _blake3_sim(combined.encode())

    # ---- CRDT merge ----

    def merge_from(self, other: "MerkleLog") -> int:
        """Merge mutations from another node.  Returns number of new entries."""
        added = 0
        for m in other.entries:
            if m.hash not in self.seen:
                self._apply(m)
                added += 1
        self.vclock.merge(other.vclock)
        return added

    @property
    def root(self) -> str:
        return self._root

    @property
    def mutation_count(self) -> int:
        return len(self.entries)


# ---------------------------------------------------------------------------
# Simulated AIMP Node
# ---------------------------------------------------------------------------

SENSOR_KEYS = ["temperature", "humidity", "pressure", "co2", "voltage"]
SENSOR_RANGES = {
    "temperature": (18.0, 45.0),
    "humidity":    (20.0, 90.0),
    "pressure":    (900.0, 1100.0),
    "co2":         (400.0, 2000.0),
    "voltage":     (3.0, 5.0),
}


class AimpNode:
    """Simulated AIMP node with a local Merkle-DAG and gossip capability."""

    def __init__(self, node_id: str) -> None:
        self.node_id = node_id
        self.log = MerkleLog(node_id)
        self.online = True

    def sense(self) -> None:
        """Read a random sensor value and append it to the local log."""
        key = random.choice(SENSOR_KEYS)
        lo, hi = SENSOR_RANGES[key]
        value = f"{random.uniform(lo, hi):.2f}"
        self.log.append(key, value)

    def gossip_to(self, peer: "AimpNode") -> int:
        """Push local state to peer.  Returns mutations transferred."""
        if not self.online or not peer.online:
            return 0
        return peer.log.merge_from(self.log)

    @property
    def root(self) -> str:
        return self.log.root

    @property
    def mutations(self) -> int:
        return self.log.mutation_count


# ---------------------------------------------------------------------------
# Simulation
# ---------------------------------------------------------------------------

COLORS = {
    "header":  "\033[1;36m",
    "ok":      "\033[1;32m",
    "warn":    "\033[1;33m",
    "err":     "\033[1;31m",
    "reset":   "\033[0m",
    "dim":     "\033[2m",
}


def _c(key: str, text: str) -> str:
    return f"{COLORS[key]}{text}{COLORS['reset']}"


def print_header(title: str) -> None:
    print()
    print(_c("header", f"{'='*60}"))
    print(_c("header", f"  {title}"))
    print(_c("header", f"{'='*60}"))


def print_round(nodes: List[AimpNode], round_num: int, label: str = "") -> None:
    suffix = f" {_c('warn', label)}" if label else ""
    print(f"\n{_c('dim', f'Round {round_num}')}{suffix}")
    roots = [n.root for n in nodes]
    all_same = len(set(roots)) == 1
    for node in nodes:
        status = _c("ok", "✓ converged") if all_same else _c("warn", "diverged")
        online = "" if node.online else _c("err", " [OFFLINE]")
        print(f"  {node.node_id:10s}  root={node.root}  mutations={node.mutations:3d}  {status}{online}")
    return all_same


def run_gossip_round(nodes: List[AimpNode]) -> None:
    """Each online node gossips to every other online node."""
    online = [n for n in nodes if n.online]
    for sender in online:
        for receiver in online:
            if sender is not receiver:
                sender.gossip_to(receiver)


def main() -> None:
    random.seed(42)
    print_header("AIMP IoT Sensor Mesh — CRDT Convergence Simulation")

    # Create 4 nodes
    nodes = [AimpNode(f"node-{c}") for c in ["A", "B", "C", "D"]]

    print("\nPhase 1: Normal operation — all nodes online")
    for r in range(1, 4):
        for node in nodes:
            node.sense()          # each node writes one sensor reading
        run_gossip_round(nodes)
        print_round(nodes, r)
        time.sleep(0.3)

    # ---- partition node-C and node-D ----
    print(f"\n{_c('err', '[PARTITION] node-C and node-D isolated from mesh')}")
    nodes[2].online = False
    nodes[3].online = False

    print("\nPhase 2: Partitioned — node-C and node-D offline")
    for r in range(4, 7):
        for node in nodes:
            node.sense()          # all nodes still produce data locally
        run_gossip_round(nodes)
        print_round(nodes, r, label="(partitioned)")
        time.sleep(0.3)

    # node-C and node-D wrote locally while offline — bring them back
    print(f"\n{_c('ok', '[HEAL] partition lifted — node-C and node-D reconnect')}")
    nodes[2].online = True
    nodes[3].online = True

    print("\nPhase 3: Healing — CRDT merge in progress")
    for r in range(7, 11):
        for node in nodes:
            node.sense()
        run_gossip_round(nodes)
        converged = print_round(nodes, r, label="(healing)")
        time.sleep(0.3)
        if converged:
            break

    roots = [n.root for n in nodes]
    if len(set(roots)) == 1:
        print(f"\n{_c('ok', '✅  All nodes converged on root ' + roots[0])}")
        print(_c("ok", "    CRDT guarantee: eventual consistency achieved!"))
    else:
        print(f"\n{_c('warn', '⚠  Nodes not yet fully converged — run more rounds')}")

    print(f"\n{_c('dim', 'Total mutations per node:')}")
    for node in nodes:
        print(f"  {node.node_id}: {node.mutations}")
    print()


if __name__ == "__main__":
    main()
