# AIMP Demo Suite

This folder contains **8 self-contained Proof-of-Concept demos** that illustrate the core capabilities of the **AIMP (AI Mesh Protocol)** from different angles.  
Each demo lives in its own subdirectory and can be run independently — no running Rust node is required unless stated otherwise.

---

## Demo Index

| # | Folder | Format | Use Case |
|---|--------|--------|----------|
| 1 | [`01_iot_sensor_mesh`](01_iot_sensor_mesh/) | Python | P2P gossip mesh with CRDT state sync between IoT sensors |
| 2 | [`02_crdt_convergence`](02_crdt_convergence/) | HTML/JS | Interactive Merkle-DAG CRDT convergence across partitioned nodes |
| 3 | [`03_wildfire_alert`](03_wildfire_alert/) | Python | Wildfire sensor network with BFT quorum consensus alerting |
| 4 | [`04_ai_rule_engine`](04_ai_rule_engine/) | Python | Pluggable rule-based AI inference engine (REPL) |
| 5 | [`05_disaster_response`](05_disaster_response/) | HTML/JS | First-responder mesh coordination visual simulator |
| 6 | [`06_cold_chain`](06_cold_chain/) | Python | Cold-chain temperature monitoring with tamper-proof audit trail |
| 7 | [`07_robot_fleet`](07_robot_fleet/) | HTML/JS | Warehouse robot fleet occupancy-map sharing |
| 8 | [`08_quorum_consensus`](08_quorum_consensus/) | Python | BFT quorum voting under Byzantine faults |

---

## Quick Start

### Python demos (1, 3, 4, 6, 8)
```bash
# No external dependencies needed — uses Python 3 standard library only
python3 demo/01_iot_sensor_mesh/simulate.py
python3 demo/03_wildfire_alert/simulate.py
python3 demo/04_ai_rule_engine/repl.py
python3 demo/06_cold_chain/simulate.py
python3 demo/08_quorum_consensus/simulate.py
```

### HTML/JS demos (2, 5, 7)
Open the HTML file directly in any modern browser — no server needed.
```bash
open demo/02_crdt_convergence/index.html
open demo/05_disaster_response/index.html
open demo/07_robot_fleet/index.html
```

---

## Protocol Overview

All demos simulate the key AIMP primitives:

| Primitive | Description |
|-----------|-------------|
| **Merkle-DAG CRDT** | Causally-ordered, append-only state with automatic conflict resolution |
| **Vector Clocks** | Happen-before tracking for each node's mutations |
| **Ed25519 Identity** | Each node has a cryptographic identity; messages are signed |
| **Gossip Protocol** | Peers exchange state diffs via UDP broadcast |
| **BFT Quorum** | Decisions require ≥ threshold nodes to agree before committing |
| **Rule Engine** | Deterministic AI inference from configurable keyword rules |

---

## Architecture Diagram

```
  Node A                Node B                Node C
  ------                ------                ------
  [Sensor]              [Sensor]              [Sensor]
     |                     |                     |
  [CRDT Actor]          [CRDT Actor]          [CRDT Actor]
     |   \gossip         / |  \gossip          / |
  [Merkle-DAG]    [Merkle-DAG]           [Merkle-DAG]
     |                     |                     |
  [AI Rule Engine]   [AI Rule Engine]   [AI Rule Engine]
     |                     |                     |
  [Quorum Vote] ---------> | <---------- [Quorum Vote]
                      [Decision]
```

Each demo exercises a different slice of this architecture.
