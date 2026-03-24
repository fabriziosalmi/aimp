# Demo 6 — Cold Chain Logistics

**Format:** Python simulation (stdlib only)  
**Use case:** Temperature sensors in refrigerated shipping containers create a tamper-proof audit trail using AIMP's encrypted persistence model.

## What it shows

- Multiple containers each run a local AIMP-style node.
- Temperature readings are appended to a **hash-chained log** (simulating ChaCha20Poly1305 encrypted redb).
- Each entry is signed with the container's Ed25519-like identity (simulated with `hmac`).
- At any point the full chain can be **verified** for integrity; tampering is detected immediately.
- A summary report is generated at the end of the shipment.

## How to run

```bash
python3 simulate.py
```

No external dependencies — Python 3.8+ standard library only.

## Expected output

```
=== AIMP Cold Chain Logistics Demo ===
Container CONT-A  [Vaccine Batch 2024-Q1]
  T+00:00  temp= 2.1°C  ✓ in range [2–8°C]   hash=a1b2c3d4
  T+01:00  temp= 2.8°C  ✓ in range [2–8°C]   hash=b2c3d4e5
  T+03:00  temp= 9.4°C  ⚠ EXCURSION          hash=c3d4e5f6
...
Audit: verifying chain integrity...  ✓ All 24 entries valid
Excursions: 2  Max temp: 9.4°C  Min temp: 1.9°C
```
