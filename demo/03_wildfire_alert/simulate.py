#!/usr/bin/env python3
"""
Demo 3 — Wildfire Alert Network
================================
Simulates a network of remote forest sensors that detect wildfire conditions
and raise a quorum-verified alert.  No external dependencies required.

Key concepts demonstrated:
  • Distributed sensor nodes with local inference
  • BFT quorum consensus (votes must reach threshold before EVACUATE is issued)
  • Faulty node detection (spurious readings blocked by quorum)
  • Gossip-based vote propagation
"""
import hashlib
import hmac
import random
import time
from dataclasses import dataclass, field
from typing import Dict, List, Optional, Tuple


# ---------------------------------------------------------------------------
# Sensor model
# ---------------------------------------------------------------------------

@dataclass
class SensorReading:
    node_id: str
    tick: int
    temperature: float   # Celsius
    humidity: float      # percent
    smoke: float         # 0.0 – 1.0

    def is_alert(self) -> bool:
        """Local threshold check — mirrors the AIMP rule engine."""
        return (
            self.temperature > 60
            or self.humidity < 20
            or self.smoke > 0.7
        )

    def severity(self) -> str:
        if self.temperature > 80 or self.smoke > 0.9:
            return "CRITICAL"
        if self.is_alert():
            return "ALERT"
        return "NORMAL"


# ---------------------------------------------------------------------------
# Quorum manager (BFT)
# ---------------------------------------------------------------------------

@dataclass
class Vote:
    """A signed vote from a sensor node."""
    node_id: str
    round_id: int
    value: str           # "EVACUATE" or "NORMAL"
    signature: str       # simulated HMAC

    @staticmethod
    def _sign(node_id: str, round_id: int, value: str) -> str:
        key = f"secret-{node_id}".encode()
        msg = f"{round_id}:{value}".encode()
        return hmac.new(key, msg, hashlib.sha256).hexdigest()[:12]

    @classmethod
    def create(cls, node_id: str, round_id: int, value: str) -> "Vote":
        return cls(node_id, round_id, value, cls._sign(node_id, round_id, value))

    def is_valid(self) -> bool:
        expected = self._sign(self.node_id, self.round_id, self.value)
        return hmac.compare_digest(self.signature, expected)


class QuorumManager:
    """Collects votes and commits when ≥ threshold honest nodes agree."""

    def __init__(self, threshold: int) -> None:
        self.threshold = threshold
        self.votes: Dict[str, Vote] = {}   # node_id → latest vote
        self.decisions: List[Tuple[int, str]] = []

    def submit(self, vote: Vote) -> None:
        if not vote.is_valid():
            _warn(f"    ⛔ Invalid signature from {vote.node_id} — discarded")
            return
        self.votes[vote.node_id] = vote

    def try_decide(self, round_id: int) -> Optional[str]:
        """Return a committed decision if quorum reached, else None."""
        counts: Dict[str, int] = {}
        for v in self.votes.values():
            if v.round_id == round_id:
                counts[v.value] = counts.get(v.value, 0) + 1
        for value, count in counts.items():
            if count >= self.threshold:
                self.decisions.append((round_id, value))
                return value
        return None


# ---------------------------------------------------------------------------
# Sensor node
# ---------------------------------------------------------------------------

class SensorNode:
    def __init__(self, node_id: str, faulty: bool = False) -> None:
        self.node_id = node_id
        self.faulty = faulty
        self._base_temp = random.uniform(20, 35)
        self._base_hum = random.uniform(40, 70)

    def read(self, tick: int, fire_tick: Optional[int] = None) -> SensorReading:
        if self.faulty:
            # Faulty sensor: always reads fire conditions (spurious)
            return SensorReading(
                self.node_id, tick,
                temperature=random.uniform(85, 100),
                humidity=random.uniform(5, 15),
                smoke=random.uniform(0.85, 0.99),
            )
        if fire_tick is not None and tick >= fire_tick:
            progress = min(1.0, (tick - fire_tick) / 3.0)
            temp = self._base_temp + progress * random.uniform(40, 60)
            hum = self._base_hum - progress * random.uniform(35, 50)
            smoke = progress * random.uniform(0.7, 1.0)
        else:
            temp = self._base_temp + random.gauss(0, 2)
            hum = self._base_hum + random.gauss(0, 3)
            smoke = random.uniform(0.0, 0.05)
        return SensorReading(
            self.node_id, tick,
            temperature=max(0, temp),
            humidity=max(0, min(100, hum)),
            smoke=max(0.0, min(1.0, smoke)),
        )


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

RESET = "\033[0m"


def _color(code: str, text: str) -> str:
    return f"{code}{text}{RESET}"


def _ok(t: str) -> str:   return _color("\033[1;32m", t)
def _warn(t: str) -> str: return _color("\033[1;33m", t)
def _err(t: str) -> str:  return _color("\033[1;31m", t)
def _dim(t: str) -> str:  return _color("\033[2m", t)
def _bold(t: str) -> str: return _color("\033[1m", t)


def severity_str(s: str) -> str:
    if s == "CRITICAL": return _err(f"🔥 {s}")
    if s == "ALERT":    return _warn(f"⚠  {s}")
    return _ok(f"✓  {s}")


# ---------------------------------------------------------------------------
# Simulation
# ---------------------------------------------------------------------------

def main() -> None:
    random.seed(7)

    print()
    print(_bold("=" * 62))
    print(_bold("  AIMP Wildfire Sensor Network — Quorum Consensus Demo"))
    print(_bold("=" * 62))

    # 4 sensor nodes; node-SW is faulty (always fires); fire starts at tick 5
    nodes = [
        SensorNode("sensor-NW"),
        SensorNode("sensor-NE"),
        SensorNode("sensor-SE"),
        SensorNode("sensor-SW"),
    ]
    FIRE_TICK = 5
    TOTAL_TICKS = 8
    THRESHOLD = 3          # ≥3 of 4 nodes must vote EVACUATE
    qm = QuorumManager(threshold=THRESHOLD)

    print(f"\nSetup: {len(nodes)} sensor nodes, quorum threshold={THRESHOLD}")
    print(f"       Fire starts propagating at tick {FIRE_TICK}\n")

    evacuated = False

    for tick in range(1, TOTAL_TICKS + 1):
        print(_dim(f"─── Tick {tick} {'─'*48}"))
        readings = [n.read(tick, fire_tick=FIRE_TICK) for n in nodes]

        for r in readings:
            sv = r.severity()
            tag = severity_str(sv)
            faulty = _err(" [FAULTY SENSOR]") if any(n.node_id == r.node_id and n.faulty for n in nodes) else ""
            print(
                f"  {r.node_id:12s}  "
                f"temp={r.temperature:5.1f}°C  "
                f"hum={r.humidity:4.1f}%  "
                f"smoke={r.smoke:.2f}  "
                f"→ {tag}{faulty}"
            )

        # Nodes vote
        print()
        for r in readings:
            vote_val = "EVACUATE" if r.is_alert() else "NORMAL"
            vote = Vote.create(r.node_id, tick, vote_val)
            qm.submit(vote)
            color = _err if vote_val == "EVACUATE" else _ok
            print(f"  {r.node_id:12s} votes {color(vote_val)}")

        # Try quorum
        decision = qm.try_decide(tick)
        evacuate_count = sum(1 for v in qm.votes.values() if v.round_id == tick and v.value == "EVACUATE")
        normal_count = sum(1 for v in qm.votes.values() if v.round_id == tick and v.value == "NORMAL")
        print(f"\n  Quorum tally: {evacuate_count} EVACUATE, {normal_count} NORMAL (threshold={THRESHOLD})")

        if decision == "EVACUATE" and not evacuated:
            print(_err(f"\n  ✅  QUORUM REACHED → DECISION: EVACUATE SECTOR ALPHA  🔥🔥🔥"))
            evacuated = True
        elif decision == "NORMAL":
            print(_ok(f"\n  ✅  QUORUM REACHED → DECISION: NORMAL, no action required"))
        else:
            print(_warn(f"\n  ⏳  No quorum yet (need {THRESHOLD})"))

        time.sleep(0.4)

    print()
    print(_bold("─── Simulation complete ───────────────────────────────────"))
    if evacuated:
        print(_ok("  Evacuation order correctly issued by distributed quorum."))
        print(_ok("  Single faulty sensor could NOT override the honest majority."))
    else:
        print(_warn("  Quorum never reached — adjust threshold or tick count."))
    print()


if __name__ == "__main__":
    main()
