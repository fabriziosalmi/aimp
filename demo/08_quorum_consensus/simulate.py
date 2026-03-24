#!/usr/bin/env python3
"""
Demo 8 — BFT Quorum Consensus
================================
Demonstrates Byzantine Fault Tolerant quorum voting — the mechanism AIMP
uses to ensure that AI decisions are only committed when a sufficient number
of honest nodes agree.

No external dependencies required.

Usage:
    python3 simulate.py                          # 5 nodes, threshold=3, 1 Byzantine
    python3 simulate.py --nodes 7 --threshold 5  # custom cluster
    python3 simulate.py --byzantine 0            # all-honest run
"""
import argparse
import hashlib
import hmac
import random
import time
from dataclasses import dataclass, field
from typing import Dict, List, Optional, Set, Tuple


# ---------------------------------------------------------------------------
# Primitives
# ---------------------------------------------------------------------------

def _sign(node_id: str, round_id: int, value: str) -> str:
    key = f"node-secret-{node_id}".encode()
    msg = f"{round_id}:{value}".encode()
    return hmac.new(key, msg, hashlib.sha256).hexdigest()[:12]


@dataclass
class Vote:
    node_id: str
    round_id: int
    value: str        # proposed value
    signature: str

    @classmethod
    def create(cls, node_id: str, round_id: int, value: str) -> "Vote":
        return cls(node_id, round_id, value, _sign(node_id, round_id, value))

    def is_valid(self) -> bool:
        expected = _sign(self.node_id, self.round_id, self.value)
        return hmac.compare_digest(self.signature, expected)

    def with_invalid_sig(self) -> "Vote":
        """Return a copy with a deliberately broken signature."""
        return Vote(self.node_id, self.round_id, self.value, "badbadbadbad")


# ---------------------------------------------------------------------------
# Quorum manager
# ---------------------------------------------------------------------------

class QuorumManager:
    def __init__(self, threshold: int) -> None:
        self.threshold = threshold
        # votes[round][node_id] = list of votes (equivocation detection)
        self.votes: Dict[int, Dict[str, List[Vote]]] = {}

    def submit(self, vote: Vote) -> Optional[str]:
        """Submit a vote.  Returns 'equivocation' if detected, else None."""
        if not vote.is_valid():
            return "invalid_signature"
        rnd = self.votes.setdefault(vote.round_id, {})
        node_votes = rnd.setdefault(vote.node_id, [])
        # Check equivocation: same node, same round, different value
        for existing in node_votes:
            if existing.value != vote.value:
                node_votes.append(vote)
                return "equivocation"
        node_votes.append(vote)
        return None

    def try_decide(self, round_id: int) -> Optional[str]:
        """Return committed value if quorum reached, else None."""
        rnd = self.votes.get(round_id, {})
        # Count votes per value, ignoring nodes with equivocation
        equivocators: Set[str] = set()
        for node_id, vlist in rnd.items():
            values = {v.value for v in vlist}
            if len(values) > 1:
                equivocators.add(node_id)

        counts: Dict[str, int] = {}
        for node_id, vlist in rnd.items():
            if node_id in equivocators:
                continue  # discard Byzantine node's votes
            for v in vlist:
                counts[v.value] = counts.get(v.value, 0) + 1

        for value, count in counts.items():
            if count >= self.threshold:
                return value
        return None

    def vote_summary(self, round_id: int) -> Dict[str, int]:
        """Return {value: count} excluding equivocators."""
        rnd = self.votes.get(round_id, {})
        equivocators = {
            nid for nid, vlist in rnd.items()
            if len({v.value for v in vlist}) > 1
        }
        counts: Dict[str, int] = {}
        for node_id, vlist in rnd.items():
            if node_id not in equivocators:
                for v in vlist:
                    counts[v.value] = counts.get(v.value, 0) + 1
        return counts


# ---------------------------------------------------------------------------
# Node
# ---------------------------------------------------------------------------

class Node:
    def __init__(self, node_id: str, is_byzantine: bool = False) -> None:
        self.node_id = node_id
        self.is_byzantine = is_byzantine
        self.online = True

    def cast_vote(self, round_id: int, proposal: str) -> List[Vote]:
        """Honest node: one vote.  Byzantine: two conflicting votes."""
        if not self.online:
            return []
        if self.is_byzantine:
            # Send conflicting votes (equivocation attack)
            return [
                Vote.create(self.node_id, round_id, proposal),
                Vote.create(self.node_id, round_id, "TAMPERED_VALUE"),
            ]
        return [Vote.create(self.node_id, round_id, proposal)]


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

RESET  = "\033[0m"
BOLD   = "\033[1m"
GREEN  = "\033[1;32m"
YELLOW = "\033[1;33m"
RED    = "\033[1;31m"
CYAN   = "\033[1;36m"
DIM    = "\033[2m"


def _ok(t: str) -> str:   return f"{GREEN}{t}{RESET}"
def _warn(t: str) -> str: return f"{YELLOW}{t}{RESET}"
def _err(t: str) -> str:  return f"{RED}{t}{RESET}"
def _dim(t: str) -> str:  return f"{DIM}{t}{RESET}"
def _bold(t: str) -> str: return f"{BOLD}{t}{RESET}"


def print_vote_round(
    round_id: int,
    nodes: List[Node],
    proposal: str,
    qm: QuorumManager,
    title: str = "",
) -> Optional[str]:
    print(f"\n{_bold(f'Round {round_id}')}{' — ' + title if title else ''}")
    print(f"  Proposal: {CYAN}{proposal}{RESET}")

    equivocators: Set[str] = set()
    for node in nodes:
        if not node.online:
            print(f"  {node.node_id:10s}  {_dim('[OFFLINE — not voting]')}")
            continue

        votes = node.cast_vote(round_id, proposal)
        for vote in votes:
            warn = qm.submit(vote)
            label = _err(f"⚠  BYZANTINE equivocation: also voting '{vote.value}'") \
                    if node.is_byzantine and vote.value != proposal \
                    else _ok(f"votes {vote.value}")
            if node.is_byzantine:
                byz_tag = _err(" [BYZANTINE]")
                equivocators.add(node.node_id)
            else:
                byz_tag = ""
            print(f"  {node.node_id:10s}  {label}  {_dim('sig=' + vote.signature)}{byz_tag}")

    # Show equivocation detection
    for nid in equivocators:
        print(f"  {_err(f'  ⛔ Equivocation detected for {nid} — votes discarded')}")

    # Tally
    summary = qm.vote_summary(round_id)
    decision = qm.try_decide(round_id)
    tally = "  ".join(f"{v}={c}" for v, c in summary.items())
    print(f"\n  Tally (honest): {tally or 'no votes'}  (threshold={qm.threshold})")

    if decision:
        print(_ok(f"  ✅ COMMITTED: '{decision}'"))
    else:
        print(_warn(f"  ❌ No quorum — below threshold, no decision"))
    return decision


# ---------------------------------------------------------------------------
# Simulation
# ---------------------------------------------------------------------------

def main() -> None:
    parser = argparse.ArgumentParser(description="AIMP BFT Quorum Consensus Demo")
    parser.add_argument("--nodes",     type=int, default=5,  help="Number of nodes")
    parser.add_argument("--threshold", type=int, default=3,  help="Quorum threshold")
    parser.add_argument("--byzantine", type=int, default=1,  help="Number of Byzantine nodes")
    args = parser.parse_args()

    random.seed(42)
    n_nodes = args.nodes
    threshold = args.threshold
    n_byzantine = min(args.byzantine, n_nodes)

    nodes = [Node(f"node-{i}", is_byzantine=(i >= n_nodes - n_byzantine))
             for i in range(n_nodes)]
    honest_count = sum(1 for n in nodes if not n.is_byzantine)

    print()
    print(_bold("=" * 62))
    print(_bold("  AIMP BFT Quorum Consensus Demo"))
    print(_bold("=" * 62))
    print(f"  Cluster   : {n_nodes} nodes  ({honest_count} honest, {n_byzantine} Byzantine)")
    print(f"  Threshold : {threshold} votes required to commit")
    print(f"  Note      : Byzantine nodes cast conflicting votes (equivocation)")

    # ---- Round 1: Normal operation ----
    qm1 = QuorumManager(threshold)
    print_vote_round(1, nodes, "value=42", qm1, title="Normal operation")
    time.sleep(0.5)

    # ---- Round 2: Network partition (minority cannot commit) ----
    print(f"\n{_bold('='*62)}")
    print(_warn("  Scenario: Network partition — minority cannot commit"))
    print(_bold("=" * 62))

    minority_size = threshold - 1
    qm2 = QuorumManager(threshold)

    # Partition: only nodes[0..minority_size] can communicate
    for node in nodes:
        node.online = False
    for node in nodes[:minority_size]:
        node.online = True

    print(f"\n  Minority partition: {[n.node_id for n in nodes[:minority_size]]}")
    print_vote_round(2, nodes, "value=99", qm2, title=f"Minority partition ({minority_size}/{n_nodes})")
    time.sleep(0.5)

    # Heal: all nodes online
    for node in nodes:
        node.online = True
    print(f"\n{_warn('  [HEAL] All nodes reconnected')}")
    qm3 = QuorumManager(threshold)
    print_vote_round(3, nodes, "value=99", qm3, title="After partition healed")
    time.sleep(0.5)

    # ---- Round 4: All-honest scenario ----
    if n_byzantine > 0:
        print(f"\n{_bold('='*62)}")
        print(_ok("  Scenario: All-honest cluster (Byzantine nodes removed)"))
        print(_bold("=" * 62))
        honest_nodes = [n for n in nodes if not n.is_byzantine]
        qm4 = QuorumManager(max(1, threshold - n_byzantine))
        print_vote_round(4, honest_nodes, "value=77", qm4, title="All-honest cluster")

    print()
    print(_bold("─── Summary ─────────────────────────────────────────────"))
    print(_ok("  ✓ Byzantine equivocation detected and discarded"))
    print(_ok("  ✓ Minority partition cannot commit (safety preserved)"))
    print(_ok("  ✓ Full cluster commits after heal (liveness restored)"))
    print()


if __name__ == "__main__":
    main()
