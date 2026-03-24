# Demo 7 — Warehouse Robot Fleet (Interactive HTML)

**Format:** Self-contained HTML/JS grid animation (no server needed)  
**Use case:** AGVs (Automated Guided Vehicles) operating in a warehouse share occupancy maps and task assignments via AIMP's local mesh — even when the central WiFi access point goes down.

## What it shows

- A 12×8 warehouse grid with shelves, charging stations, and pick tasks.
- 4 robots (AGVs) autonomously navigate to tasks using a greedy path planner.
- Each robot maintains a **local occupancy map** (CRDT key-value store).
- **"Sync Occupancy Maps"** merges all robots' maps (CRDT merge — only newer entries win).
- **"Toggle WiFi Outage"** simulates infrastructure failure; robots continue via peer mesh.
- Collision conflicts are tracked; robots reroute around each other.
- The simulation auto-steps every 800 ms.

## How to run

```bash
open demo/07_robot_fleet/index.html
# or double-click the file in your file manager
```

No build step, no npm, no server.

## Suggested walkthrough

1. Watch robots auto-navigate to pick tasks.
2. Click **Sync Occupancy Maps** to see CRDT merge counts.
3. Click **Toggle WiFi Outage** — robots continue moving (mesh-only mode).
4. Click **Add Pick Task** to add more work to the queue.
5. Observe how **conflict count** increases when robots try to occupy the same cell.
