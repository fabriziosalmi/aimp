#!/usr/bin/env python3
"""
Demo 6 — Cold Chain Logistics
==============================
Simulates temperature sensors in refrigerated shipping containers creating
a tamper-proof hash-chained audit trail — modelling AIMP's encrypted
persistence layer (redb + ChaCha20Poly1305).

No external dependencies required.

Key concepts demonstrated:
  • Hash-chained log (immutable, append-only)
  • Entry signing with node identity (HMAC as Ed25519 stand-in)
  • Tamper detection (any modification breaks the chain)
  • Multi-container fleet with per-container audit reports
"""
import hashlib
import hmac
import random
import time
from dataclasses import dataclass, field
from typing import List, Optional, Tuple


# ---------------------------------------------------------------------------
# Simulated encrypted entry (hash chain)
# ---------------------------------------------------------------------------

@dataclass
class ChainEntry:
    seq: int
    timestamp: str       # ISO-like string (simulated)
    container_id: str
    temperature: float
    prev_hash: str
    signature: str       # HMAC(node_secret, entry_content)
    entry_hash: str      # SHA-256 of this entry (merkle node)

    @property
    def in_range(self) -> bool:
        """Cold chain for vaccines: 2–8 °C."""
        return 2.0 <= self.temperature <= 8.0

    @property
    def status(self) -> str:
        if self.temperature < 0:
            return "FROZEN"
        if self.temperature > 8:
            return "EXCURSION_HIGH"
        if self.temperature < 2:
            return "EXCURSION_LOW"
        return "OK"


def _sign(secret: str, data: str) -> str:
    key = secret.encode()
    return hmac.new(key, data.encode(), hashlib.sha256).hexdigest()[:16]


def _entry_hash(entry: ChainEntry) -> str:
    blob = (
        f"{entry.seq}:{entry.timestamp}:{entry.container_id}:"
        f"{entry.temperature}:{entry.prev_hash}:{entry.signature}"
    )
    return hashlib.sha256(blob.encode()).hexdigest()[:16]


# ---------------------------------------------------------------------------
# Container node
# ---------------------------------------------------------------------------

GENESIS_HASH = "0" * 16


class ContainerNode:
    """An AIMP-style node embedded in a refrigerated container."""

    def __init__(self, container_id: str, cargo_label: str) -> None:
        self.container_id = container_id
        self.cargo_label = cargo_label
        self._secret = f"key-{container_id}"   # simulated per-node private key
        self._chain: List[ChainEntry] = []

    def record(self, hour: int, temperature: float) -> ChainEntry:
        seq = len(self._chain) + 1
        ts = f"2024-01-15T{hour:02d}:00:00Z"
        prev_hash = self._chain[-1].entry_hash if self._chain else GENESIS_HASH
        sig_content = f"{seq}:{ts}:{self.container_id}:{temperature:.2f}:{prev_hash}"
        signature = _sign(self._secret, sig_content)
        entry = ChainEntry(
            seq=seq,
            timestamp=ts,
            container_id=self.container_id,
            temperature=temperature,
            prev_hash=prev_hash,
            signature=signature,
            entry_hash="",   # filled below
        )
        entry.entry_hash = _entry_hash(entry)
        self._chain.append(entry)
        return entry

    def verify_chain(self) -> Tuple[bool, List[int]]:
        """Verify hash chain integrity.  Returns (ok, list_of_bad_seqs)."""
        bad = []
        expected_prev = GENESIS_HASH
        for entry in self._chain:
            # Check prev_hash linkage
            if entry.prev_hash != expected_prev:
                bad.append(entry.seq)
            # Recompute entry hash
            recomputed = _entry_hash(entry)
            if recomputed != entry.entry_hash:
                bad.append(entry.seq)
            expected_prev = entry.entry_hash
        return (len(bad) == 0, bad)

    def tamper(self, seq: int, new_temp: float) -> None:
        """Simulate tampering with an entry (for demo purposes)."""
        for entry in self._chain:
            if entry.seq == seq:
                entry.temperature = new_temp
                break

    @property
    def chain(self) -> List[ChainEntry]:
        return self._chain


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


def _status_str(s: str) -> str:
    if s == "OK":              return f"{GREEN}✓  OK{RESET}"
    if s.startswith("EXCURS"): return f"{RED}⚠  {s}{RESET}"
    if s == "FROZEN":          return f"{YELLOW}❄  {s}{RESET}"
    return s


def simulate_temperatures(hours: int, excursion_hour: Optional[int] = None) -> List[float]:
    """Simulate a realistic temperature profile for a container."""
    temps = []
    for h in range(hours):
        base = random.gauss(4.5, 0.5)
        if excursion_hour and h == excursion_hour:
            base += random.uniform(5, 8)   # door opened / cooling failure
        elif excursion_hour and h == excursion_hour + 1:
            base += random.uniform(1, 3)   # still recovering
        temps.append(round(base, 2))
    return temps


# ---------------------------------------------------------------------------
# Simulation
# ---------------------------------------------------------------------------

def run_container(node: ContainerNode, temps: List[float]) -> None:
    print(f"\n  {BOLD}Container {node.container_id}{RESET}  [{node.cargo_label}]")
    print(f"  {'─'*60}")
    for hour, temp in enumerate(temps):
        entry = node.record(hour, temp)
        status = entry.status
        print(
            f"  T+{hour:02d}:00  "
            f"temp={temp:5.2f}°C  "
            f"{_status_str(status):30s}  "
            f"{DIM}hash={entry.entry_hash}{RESET}"
        )
        time.sleep(0.05)


def audit_container(node: ContainerNode, tamper: bool = False) -> None:
    chain = node.chain
    if tamper and chain:
        idx = len(chain) // 2
        old_temp = chain[idx].temperature
        node.tamper(chain[idx].seq, old_temp + 15.0)
        print(f"\n  {RED}[TAMPER INJECTED]{RESET} seq={chain[idx].seq}  "
              f"temp changed from {old_temp}→{chain[idx].temperature}")

    ok, bad = node.verify_chain()
    excursions = [e for e in chain if not e.in_range]
    max_temp = max(e.temperature for e in chain)
    min_temp = min(e.temperature for e in chain)

    print(f"\n  {BOLD}Audit: {node.container_id}{RESET}")
    if ok:
        print(f"  Chain integrity : {GREEN}✓ All {len(chain)} entries valid{RESET}")
    else:
        print(f"  Chain integrity : {RED}✗ TAMPERED — bad entries: {bad}{RESET}")
    print(f"  Excursions      : {len(excursions)}")
    print(f"  Temp range      : {min_temp:.2f}°C – {max_temp:.2f}°C")
    print(f"  Entries         : {len(chain)}")


def main() -> None:
    random.seed(99)

    print()
    print(BOLD + "=" * 62 + RESET)
    print(BOLD + "  AIMP Cold Chain Logistics — Tamper-Proof Audit Trail" + RESET)
    print(BOLD + "=" * 62 + RESET)
    print(f"\n  Simulating 24-hour shipment with 2 containers")
    print(f"  Cold chain requirement: 2–8 °C (vaccine grade)\n")

    # Container A — clean shipment
    cont_a = ContainerNode("CONT-A", "Vaccine Batch 2024-Q1")
    temps_a = simulate_temperatures(24, excursion_hour=None)
    run_container(cont_a, temps_a)

    # Container B — has a temperature excursion at hour 14
    cont_b = ContainerNode("CONT-B", "Insulin Batch 2024-Q2")
    temps_b = simulate_temperatures(24, excursion_hour=14)
    run_container(cont_b, temps_b)

    # Audit
    print(f"\n{BOLD}{'='*62}{RESET}")
    print(f"{BOLD}  Audit Report{RESET}")
    print(f"{BOLD}{'='*62}{RESET}")

    audit_container(cont_a, tamper=False)
    audit_container(cont_b, tamper=False)

    # Demo: inject tampering in container A
    print(f"\n{BOLD}{'='*62}{RESET}")
    print(f"{BOLD}  Tamper Detection Demo (container CONT-A){RESET}")
    print(f"{BOLD}{'='*62}{RESET}")
    audit_container(cont_a, tamper=True)

    print()


if __name__ == "__main__":
    main()
