# AIMP (AI Mesh Protocol) v0.1.0

**[Paper on ResearchGate](https://www.researchgate.net/publication/403127328_AIMP_AI_Mesh_Protocol_Design_and_Evaluation_of_a_Serverless_Merkle-CRDT_Protocol_for_Edge_Agent_Synchronization)**

**AIMP** is an experimental, serverless networking protocol designed for resilient state synchronization between autonomous agents in fragmented, low-bandwidth networks.

Unlike traditional cloud-based protocols, AIMP operates on a **Local-First** principle, utilizing Merkle-CRDTs and cryptographic identity to ensure eventual consistency without a central authority or global DNS.

---

## Architecture

```
aimp_node/          Rust reference implementation (Cargo workspace member)
  src/
    crdt/           Merkle-DAG engine, actor model, arena allocator, quorum consensus
    crypto/         Ed25519 identity, BLAKE3 hashing, zero-trust firewall
    network/        UDP gossip, Noise Protocol XX sessions, per-peer rate limiting
    protocol/       Wire format (MessagePack), typed payload enum
    decision_engine.rs  Pluggable deterministic decision engine (trait + rule engine + hot-reload)
    error.rs        Unified AimpError type hierarchy
    dashboard/      Ratatui TUI
    config.rs       Dynamic configuration with validation
    event/          Structured logging + Prometheus metrics (counters + histograms)
  tests/            Integration tests
  benches/          Criterion benchmarks
aimp_testbed/       Python SDK (aimp-client) + CLI tool + chaos testing
deploy/             Systemd service, Firecracker microVM, install script
formal/             TLA+ convergence + quorum safety specification
docs/               Whitepaper (Typst source + PDF)
```

## Strategic Advantages

| Feature          | AIMP (Merkle-CRDT)         | Traditional (Raft/Paxos) |
| ---------------- | -------------------------- | ------------------------ |
| **Topology**     | P2P Mesh / Decentralized   | Leader / Quorum          |
| **Availability** | AP (Always Writeable)      | CP (Requires Majority)   |
| **Ordering**     | Causal (Vector Clocks)     | Total (Sequential)       |
| **Integrity**    | Cryptographic (Merkle-DAG) | Log-based                |
| **Hardware**     | Edge/IoT Optimized         | Data Center Grade        |

## Key Features (v0.1.0)

**Core Engine**
- Actor Model with zero-shared-state CRDT via `tokio::mpsc`
- Slab/Arena allocation with O(1) insertion and SoA layout
- Durable persistence via redb with ChaCha20Poly1305 encryption at rest
- HKDF-SHA256 key derivation with domain separation
- Cached merkle root with invalidation-on-write
- Real mark-and-sweep GC with slab memory reclamation
- Epoch-based GC tracking integrated into the CRDT actor

**Networking & Security**
- Noise Protocol XX encrypted sessions (default on)
- Per-peer token bucket rate limiting (integer arithmetic)
- O(1) gossip deduplication via HashSet + VecDeque
- TTL replay attack detection with circuit breaker
- Session LRU eviction (TTL + max count)
- Protocol version range negotiation for rolling upgrades

**Decision Engine & Consensus**
- Pluggable `DecisionEngine` trait with `RuleEngine` implementation
- Hot-reload rules from `aimp_rules.json` (no restart needed)
- BFT quorum voting with persistent verified decisions
- Typed `Payload` enum per opcode (compile-time safety)

**Observability**
- Prometheus counters, gauges, and latency histograms
- Composite `/health` endpoint with sub-checks and HTTP status codes
- Structured `SystemEvent` logging with TUI dashboard

**Operations**
- Unified `AimpError` type hierarchy (no more `Box<dyn Error>`)
- Config validation (rejects invalid parameter combinations)
- Graceful shutdown with 5-second timeout
- Systemd hardened service file
- CI/CD: lint, test, security audit, docs, cross-compiled releases

## Benchmarks

Measured with Criterion on Apple Silicon (M-series), single-threaded, `fast-crypto` mode:

| Operation | Time | Throughput |
|-----------|------|------------|
| `append_mutation` (100 ops) | 41.8 µs | ~2.4M mutations/sec |
| `get_merkle_root` (cached) | 4.8 ns | O(1) |
| BLAKE3 hash (1 KB) | 925 ns | ~1.08 GB/s |
| MessagePack ser / de | 204 / 210 ns | — |
| Ed25519 sign (ring) | 9.3 µs | ~108K ops/sec |
| Ed25519 verify | 25.0 µs | ~40K ops/sec |

### System-Level Benchmarks

Simulated 5-node cluster with anti-entropy sync (in-process, Apple Silicon):

| Scenario | Result |
|----------|--------|
| Throughput (5 nodes x 1000 mutations, with Ed25519 sign) | **96,289 mutations/sec** |
| Convergence (5 divergent nodes, 250 DAG each) | **0.68 ms** (1 sync round) |
| Partition/Merge (2 groups, 30 mutations/group, full merge) | **0.21 ms** |
| Crypto hot-path (sign + verify per message) | 45.0 µs → **22K msg/sec max** |
| Crypto budget at rate_limit=50/sec | **0.23%** utilization |

### Network Impairment (netem simulation)

Convergence under simulated packet loss, latency, and partitions (5 nodes, 50 mutations/node):

| Condition | Converged | Rounds |
|-----------|-----------|--------|
| Baseline (0% loss) | YES | 1 |
| 10% packet loss | YES | 2 |
| 30% packet loss | YES | 2 |
| 50% packet loss | YES | 2 |
| 20% loss + 100ms latency + 30ms jitter | YES | 2 |
| Partition (10 rounds) then merge | YES | 1 |
| Partition (50 rounds) then merge with 20% loss | YES | 1 |
| 80% packet loss (stress) | YES | 4 |

AIMP converges up to ~80% packet loss within a few anti-entropy rounds, degrading gracefully.

### Cross-Platform (ARM64 resource-constrained)

Docker ARM64 Linux with RPi-class resource limits:

| Metric | macOS ARM64 | Linux 1C/1GB (RPi 4) | Linux 1C/256MB (RPi Zero) |
|--------|-------------|----------------------|--------------------------|
| Throughput | 96,289 mut/s | 24,802 mut/s | 29,709 mut/s |
| Convergence | 0.68 ms | 3.06 ms | 1.30 ms |
| Ed25519 sign | 8.7 µs | 16.2 µs | 15.1 µs |
| Ed25519 verify | 20.5 µs | 34.6 µs | 45.2 µs |
| Max msg/sec | 34,329 | 19,695 | 16,573 |
| Crypto budget @50/s | 0.15% | 0.25% | 0.30% |

Even on RPi Zero class hardware, throughput is 3 orders of magnitude above the rate limit.

### Comparison with Automerge v0.7

Same hardware, same operations, single-threaded, `target-cpu=native`:

| Benchmark | AIMP (ring) | Automerge | Yrs (Yjs) |
|-----------|------------|-----------|-----------|
| Mutation (1000 ops) | **129K ops/s** | 94K ops/s | 632K ops/s |
| 2-replica merge | **0.48 ms** | 1.17 ms | 0.38 ms |
| 5-replica merge | **2.16 ms** | 3.89 ms | — |
| State size (1000 ops) | ~18 KB | 4 KB | — |

AIMP with `ring` outperforms Automerge by 1.37x on mutations (with Ed25519 per write) and 2.4x on merge. Yrs is fastest on mutation (no crypto) but AIMP merge is within 26% of Yrs.

```bash
# Enable ring backend for maximum throughput
RUSTFLAGS="-C target-cpu=native" cargo run --release --features fast-crypto,fast-alloc
```

Run benchmarks locally:
```bash
cargo bench --manifest-path aimp_node/Cargo.toml             # Micro-benchmarks
cargo run --release -p aimp_node --example bench_convergence  # System benchmarks
cargo run --release -p aimp_node --example bench_netem        # Network impairment
docker build -f Dockerfile.bench -t aimp-bench . && \
  docker run --rm --memory=1g --cpus=1 aimp-bench             # ARM64 constrained
```

## Formal Verification (TLA+)

The protocol's core safety properties are formally specified in `formal/AimpCrdtConvergence.tla` and verified with the TLC model checker:

| Property | Description | Status |
|----------|-------------|--------|
| **Convergence** | If two nodes possess the same store, they compute the same Merkle heads | Verified |
| **QuorumSafety** | If quorum is reached for a prompt, the decision is unique (no conflicting decisions) | Verified |
| **QuorumLiveness** | If all nodes vote for the same decision, the quorum threshold is eventually reached | Verified |

TLC explored **46,063 states** (9,558 distinct) to depth 16 in <1 second with 10 parallel workers and zero violations. Model parameters: 2 nodes, 3 mutations/node, 2 prompts, 2 decisions, quorum threshold = 2.

**Bugs found during verification:** TLC uncovered 2 correctness bugs in the original spec (also present in the Rust code): (1) out-of-order Receive producing incorrect heads, and (2) quorum double-voting allowing conflicting decisions. Both fixed in the TLA+ spec and Rust implementation.

## Edge Deployment

AIMP is designed to run as a **single static binary** with zero runtime dependencies. No Docker, no container runtime, no JVM.

### Quick Deploy (bare metal)

```bash
# Download the binary for your architecture
curl -LO https://github.com/fabriziosalmi/aimp/releases/latest/download/aimp_node-aarch64-linux
chmod +x aimp_node-aarch64-linux

# Install as systemd service
sudo deploy/install.sh ./aimp_node-aarch64-linux

# Start
sudo systemctl start aimp-node
curl localhost:9090/health
```

### Cross-Compile from Source

```bash
make install-cross-targets   # One-time: install musl targets
make edge-arm64              # ARM64 (RPi 4/5, Jetson, Graviton)
make edge-armv7              # ARMv7 (RPi 2/3, industrial PLCs)
make edge-x86                # x86_64 (edge gateways)
make edge-all                # All three
```

### Firecracker MicroVM (multi-tenant isolation)

For edge gateways running multiple untrusted workloads:

```bash
sudo make microvm-rootfs     # Builds ~15MB Alpine rootfs with AIMP
firecracker --no-api --config-file deploy/firecracker/vm-config.json
```
Boot time: ~125ms. Memory: 64MB. vCPU: 1.

### Systemd Service

The included service file (`deploy/systemd/aimp-node.service`) provides:

| Hardening | Value |
|-----------|-------|
| User isolation | Dedicated `aimp` user, no login shell |
| Filesystem | `ProtectSystem=strict`, `ProtectHome=yes` |
| Memory limit | `MemoryMax=128M` |
| CPU limit | `CPUQuota=80%` |
| Privilege | `NoNewPrivileges=yes`, `MemoryDenyWriteExecute=yes` |
| Syscall filter | `@system-service` whitelist |
| Restart | On failure with exponential backoff |
| Shutdown | SIGTERM → 10s grace → SIGKILL |

## Data Flow

```mermaid
graph TD
    UDP[UDP Socket] -->|Envelope| RL[Rate Limiter]
    RL -->|Allowed| NP[Noise Protocol]
    NP -->|Decrypt| FW[Security Firewall]
    FW -->|Valid| BP[Backpressure Semaphore]
    BP -->|Permit| Parser[Protocol Parser]
    Parser -->|AimpData| CRDT[CRDT Actor]
    CRDT -->|Mutation| DAG[Merkle-DAG + redb]
    DAG -->|Prune| GC[Epoch GC]
    CRDT -->|Evaluation Req| DE[Decision Engine]
    DE -->|Decision + Evidence| CRDT
    CRDT -->|Quorum Vote| QM[QuorumManager]
```

---

## Quick Start

### 1. Run the Node
```bash
cargo run -- --port 1337 --name node1
```

### 2. Python CLI
```bash
cd aimp_testbed
pip install -e .
aimp-cli health --target 127.0.0.1 --metrics-port 9090
aimp-cli infer "Check valve pressure in sector north"
```

### 3. Run Tests & Benchmarks
```bash
make test                     # Property-based + integration tests
make bench                    # Criterion benchmarks
make lint                     # Format + clippy
make docs                     # Generate rustdoc
```

## Configuration

Configuration is loaded from (highest priority first):

1. **CLI arguments** (`--port`, `--name`)
2. **Environment variables** (`AIMP_PORT`, `AIMP_NOISE_REQUIRED`, `AIMP_PEER_RATE_LIMIT`, ...)
3. **`aimp.toml`** file (optional)
4. **Hardcoded defaults**

| Parameter | Default | Description |
|-----------|---------|-------------|
| `port` | 1337 | UDP listen port |
| `metrics_port` | 9090 | Prometheus HTTP port |
| `noise_required` | true | Enforce Noise Protocol encryption |
| `peer_rate_limit` | 50 | Max messages/sec per peer |
| `peer_rate_burst` | 100 | Token bucket burst capacity |
| `gc_mutation_threshold` | 1000 | Mutations before GC sweep |
| `quorum_threshold` | 2 | Nodes required for BFT consensus |
| `dag_history_depth` | 100 | Max DAG depth retained after GC |

## Related Work

AIMP builds on concepts from the following areas of distributed systems research:

- **CRDTs** — Shapiro et al., "A Comprehensive Study of Convergent and Commutative Replicated Data Types" (INRIA, 2011)
- **Merkle-CRDTs** — Kleppmann & Howard, "Byzantine Eventual Consistency and the Fundamental Limits of Peer-to-Peer Databases" (2022)
- **BFT Consensus** — Castro & Liskov, "Practical Byzantine Fault Tolerance" (OSDI, 1999)
- **Noise Protocol** — Perrin, "The Noise Protocol Framework" (2018); used via the `snow` crate for XX handshake pattern
- **Gossip Protocols** — Demers et al., "Epidemic Algorithms for Replicated Database Maintenance" (1987)
- **Merkle Trees** — Merkle, "A Digital Signature Based on a Conventional Encryption Function" (CRYPTO, 1987)
- **Vector Clocks** — Mattern, "Virtual Time and Global States of Distributed Systems" (1988)

## License

MIT License.
