#!/usr/bin/env python3
"""
Demo 4 — AI Rule Engine REPL
==============================
A faithful Python port of the AIMP RuleEngine (ai_bridge.rs) with an
interactive REPL that supports hot-reload of rules from a JSON file.

No external dependencies required.

Usage:
    python3 repl.py
    python3 repl.py --rules my_rules.json
"""
import argparse
import hashlib
import json
import os
import sys
from dataclasses import dataclass, field
from typing import List, Optional


# ---------------------------------------------------------------------------
# Data types (mirrors ai_bridge.rs)
# ---------------------------------------------------------------------------

@dataclass
class AiDecision:
    target_entity: str
    status: str
    action_required: bool

    def __str__(self) -> str:
        req = "yes" if self.action_required else "no"
        return (
            f"AiDecision("
            f"target='{self.target_entity}', "
            f"status='{self.status}', "
            f"action_required={req})"
        )


@dataclass
class InferenceRule:
    keywords: List[str]
    target: str
    status: str
    action_required: bool


# ---------------------------------------------------------------------------
# Rule engine (mirrors RuleEngine in ai_bridge.rs)
# ---------------------------------------------------------------------------

DEFAULT_RULES: List[dict] = [
    {
        "keywords": ["error", "failure", "fault", "critical", "danger"],
        "target": "system_alert",
        "status": "CRITICAL",
        "action_required": True,
    },
    {
        "keywords": ["valve", "pressure", "flow"],
        "target": "hydraulic_system",
        "status": "WARNING",
        "action_required": True,
    },
    {
        "keywords": ["north", "nord"],
        "target": "sector_north",
        "status": "NORMAL",
        "action_required": False,
    },
    {
        "keywords": ["south", "sud"],
        "target": "sector_south",
        "status": "NORMAL",
        "action_required": False,
    },
]


def _hash_content(content: str) -> str:
    """Return a short hash of a string (mirrors SecurityFirewall::hash)."""
    return hashlib.sha256(content.encode()).hexdigest()[:8]


class RuleEngine:
    """Keyword-based deterministic rule engine."""

    def __init__(self, rules: List[InferenceRule], version: str) -> None:
        self.rules = rules
        self.version = version

    @classmethod
    def from_default(cls) -> "RuleEngine":
        rules = [InferenceRule(**r) for r in DEFAULT_RULES]
        return cls(rules, "rules.v2.default")

    @classmethod
    def from_file(cls, path: str) -> Optional["RuleEngine"]:
        try:
            with open(path) as f:
                content = f.read()
            data = json.loads(content)
            rules = [InferenceRule(**r) for r in data]
            version = f"rules.file.{_hash_content(content)}"
            return cls(rules, version)
        except Exception as e:
            print(f"\033[1;33m[WARN] Could not load rules from {path}: {e}\033[0m",
                  file=sys.stderr)
            return None

    @property
    def model_hash(self) -> str:
        return _hash_content(self.version)

    def infer(self, prompt: str) -> AiDecision:
        pl = prompt.lower()
        for rule in self.rules:
            if any(kw in pl for kw in rule.keywords):
                return AiDecision(rule.target, rule.status, rule.action_required)
        return AiDecision("generic_entity", "NORMAL", False)


# ---------------------------------------------------------------------------
# Hot-reload wrapper
# ---------------------------------------------------------------------------

class AiEngine:
    def __init__(self, rules_path: Optional[str] = None) -> None:
        self._path = rules_path
        self._mtime: Optional[float] = None
        self._engine = self._load()

    def _load(self) -> RuleEngine:
        if self._path and os.path.exists(self._path):
            engine = RuleEngine.from_file(self._path)
            if engine:
                self._mtime = os.path.getmtime(self._path)
                return engine
        return RuleEngine.from_default()

    def try_reload(self) -> bool:
        """Reload rules if the file changed.  Returns True if reloaded."""
        if not self._path or not os.path.exists(self._path):
            return False
        mtime = os.path.getmtime(self._path)
        if mtime == self._mtime:
            return False
        new_engine = RuleEngine.from_file(self._path)
        if new_engine:
            self._engine = new_engine
            self._mtime = mtime
            return True
        return False

    def infer(self, prompt: str) -> AiDecision:
        self.try_reload()
        return self._engine.infer(prompt)

    @property
    def version(self) -> str:
        return self._engine.version

    @property
    def model_hash(self) -> str:
        return self._engine.model_hash


# ---------------------------------------------------------------------------
# REPL
# ---------------------------------------------------------------------------

RESET = "\033[0m"
BOLD  = "\033[1m"
GREEN = "\033[1;32m"
YELLOW= "\033[1;33m"
RED   = "\033[1;31m"
CYAN  = "\033[1;36m"
DIM   = "\033[2m"


def _status_color(status: str) -> str:
    if status == "CRITICAL": return f"{RED}{status}{RESET}"
    if status == "WARNING":  return f"{YELLOW}{status}{RESET}"
    return f"{GREEN}{status}{RESET}"


def _print_decision(decision: AiDecision) -> None:
    status = _status_color(decision.status)
    req = f"{RED}YES{RESET}" if decision.action_required else f"{GREEN}no{RESET}"
    print(f"  → target={CYAN}{decision.target_entity}{RESET}  "
          f"status={status}  "
          f"action_required={req}")


def print_banner(engine: AiEngine) -> None:
    print(f"\n{BOLD}{'='*58}{RESET}")
    print(f"{BOLD}  AIMP Rule Engine REPL{RESET}")
    print(f"{BOLD}{'='*58}{RESET}")
    print(f"  rules version : {CYAN}{engine.version}{RESET}")
    print(f"  model hash    : {DIM}{engine.model_hash}{RESET}")
    print(f"\n{DIM}  Commands:{RESET}")
    print(f"  {CYAN}reload{RESET}  — hot-reload rules from file")
    print(f"  {CYAN}rules{RESET}   — list active rules")
    print(f"  {CYAN}help{RESET}    — show this banner")
    print(f"  {CYAN}quit{RESET}    — exit")
    print(f"\n  {DIM}Enter any free-text prompt to run inference.{RESET}\n")


def cmd_rules(engine: AiEngine) -> None:
    rules = engine._engine.rules
    print(f"\n  {len(rules)} active rules:\n")
    for i, r in enumerate(rules, 1):
        kws = ", ".join(r.keywords)
        req = "yes" if r.action_required else "no"
        print(f"  {i}. keywords=[{kws}]")
        print(f"     → target={r.target}  status={r.status}  action_required={req}")
    print()


def cmd_reload(engine: AiEngine) -> None:
    reloaded = engine.try_reload()
    if reloaded:
        print(f"  {GREEN}✓ Rules reloaded{RESET}  version={engine.version}  hash={engine.model_hash}")
    else:
        print(f"  {DIM}No changes detected (hash={engine.model_hash}){RESET}")


SAMPLE_PROMPTS = [
    "Check valve pressure in sector north",
    "Critical failure in reactor coolant loop",
    "All systems nominal, no alerts",
    "Flow sensor anomaly in south pipeline",
    "Temperature spike detected — danger zone",
]


def main() -> None:
    parser = argparse.ArgumentParser(description="AIMP Rule Engine REPL")
    parser.add_argument("--rules", default=None,
                        help="Path to rules JSON file (hot-reload supported)")
    args = parser.parse_args()

    engine = AiEngine(rules_path=args.rules)
    print_banner(engine)

    # Show a few sample prompts on first run
    print(f"  {DIM}Sample prompts:{RESET}")
    for p in SAMPLE_PROMPTS:
        d = engine.infer(p)
        print(f"  {DIM}> {p}{RESET}")
        _print_decision(d)
    print()

    try:
        while True:
            try:
                prompt = input(f"{CYAN}>{RESET} ").strip()
            except EOFError:
                break
            if not prompt:
                continue
            if prompt.lower() in ("quit", "exit", "q"):
                print("Bye.")
                break
            if prompt.lower() == "reload":
                cmd_reload(engine)
            elif prompt.lower() == "rules":
                cmd_rules(engine)
            elif prompt.lower() in ("help", "?"):
                print_banner(engine)
            else:
                decision = engine.infer(prompt)
                _print_decision(decision)
    except KeyboardInterrupt:
        print("\nBye.")


if __name__ == "__main__":
    main()
