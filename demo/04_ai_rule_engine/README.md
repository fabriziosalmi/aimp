# Demo 4 — AI Rule Engine REPL

**Format:** Python interactive REPL (stdlib only)  
**Use case:** The AIMP pluggable rule-based AI inference engine, reimplemented in Python for interactive exploration.

## What it shows

- A faithful Python port of the `RuleEngine` from `aimp_node/src/ai_bridge.rs`.
- Rules are loaded from `rules.json` (hot-reload supported — edit the file while the REPL is running).
- Each prompt is matched against keyword rules; the first match wins.
- Outputs `AiDecision { target_entity, status, action_required }`.

## How to run

```bash
python3 repl.py
# Or with a custom rules file:
python3 repl.py --rules my_rules.json
```

No external dependencies — Python 3.8+ standard library only.

## Example session

```
AIMP Rule Engine REPL  (rules v=rules.v2.default, hash=a1b2c3d4)
Type a mesh prompt, or 'reload' to hot-reload rules, 'quit' to exit.

> Check valve pressure in sector north
→ AiDecision(target='hydraulic_system', status='WARNING', action_required=True)

> All systems nominal
→ AiDecision(target='generic_entity', status='NORMAL', action_required=False)

> Critical failure in reactor
→ AiDecision(target='system_alert', status='CRITICAL', action_required=True)

> reload
Rules reloaded (hash=a1b2c3d4, unchanged)

> quit
Bye.
```
