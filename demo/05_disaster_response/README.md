# Demo 5 — Disaster Response Coordination (Interactive HTML)

**Format:** Self-contained HTML5 Canvas animation (no server needed)  
**Use case:** First-responder teams deploy portable AIMP nodes in a disaster zone. Data merges automatically as teams enter and leave each other's radio range.

## What it shows

- A live canvas with animated team nodes drifting around the scene.
- Each team node has a **radio range circle**; overlapping ranges indicate mesh connectivity.
- Teams share building assessment reports, casualty data, and resource status.
- The CRDT merge is triggered by **"Sync All In-Range"** — only teams within radio range exchange state.
- **"Toggle Hazard Zone"** activates a danger area; teams inside turn orange.
- The event log shows exactly which teams exchanged which data.

## How to run

```bash
open demo/05_disaster_response/index.html
# or double-click the file in your file manager
```

No build step, no npm, no server.

## Suggested walkthrough

1. Click **Deploy New Team** 4–5 times to add responder teams.
2. Watch the teams drift — they're broadcasting (circles overlap when in range).
3. Click **Sync All In-Range** to merge data between nearby teams.
4. Toggle the hazard zone on/off and observe teams changing status.
5. Deploy more teams and watch the event log fill with merge events.
