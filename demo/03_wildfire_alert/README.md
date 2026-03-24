# Demo 3 — Wildfire Alert Network

**Format:** Python simulation (stdlib only)  
**Use case:** A network of remote forest sensors that detect wildfire conditions and raise a quorum-verified alert.

## What it shows

- Each sensor periodically reads temperature, humidity, and smoke-level values.
- When local readings cross thresholds an **ALERT vote** is broadcast to peers.
- The **BFT quorum** module requires ≥ 2/3 of active nodes to agree before committing the `EVACUATE` decision.
- If one sensor is faulty (sends spurious readings) the quorum still blocks false positives.

## How to run

```bash
python3 simulate.py
```

No external dependencies — Python 3.8+ standard library only.

## Expected output

```
=== AIMP Wildfire Sensor Network ===
Tick 1  sensor-NW  temp=28°C  humidity=62%  smoke=0.02  → NORMAL
Tick 1  sensor-NE  temp=31°C  humidity=55%  smoke=0.05  → NORMAL
...
Tick 5  sensor-SW  temp=67°C  humidity=18%  smoke=0.84  → ALERT  🔥
Tick 5  sensor-NW  temp=71°C  humidity=14%  smoke=0.91  → ALERT  🔥
Quorum votes: 3/4 (threshold=3) → ✅ DECISION: EVACUATE SECTOR ALPHA
```
