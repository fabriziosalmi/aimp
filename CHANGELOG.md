# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-03-23

### Added

**Core Engine**
- Merkle-CRDT synchronization engine with Actor Model (zero-shared state).
- Slab/Arena allocation with O(1) insertion and SoA layout.
- Durable persistence via redb with ChaCha20Poly1305 encryption at rest.
- HKDF-SHA256 key derivation with domain separation.
- Cached merkle root with invalidation-on-write.
- Mark-and-sweep GC with slab memory reclamation.
- Epoch-based GC tracking integrated into the CRDT actor.
- Property-based testing with `proptest` and saved regression seeds.

**Networking & Security**
- UDP gossip with Noise Protocol XX encrypted sessions (default on).
- Per-peer token bucket rate limiting (integer arithmetic).
- O(1) gossip deduplication via HashSet + VecDeque.
- TTL replay attack detection with circuit breaker.
- Session LRU eviction (TTL + max count).
- Protocol version range negotiation for rolling upgrades.
- Ed25519 identity with zero-trust signature verification.
- BLAKE3 hashing for Merkle-DAG nodes.

**AI & Consensus**
- Pluggable `InferenceEngine` trait with `RuleEngine` implementation.
- Hot-reload rules from `aimp_rules.json` (no restart needed).
- BFT quorum voting with persistent verified decisions.
- Typed `Payload` enum per opcode (compile-time safety).

**Observability**
- Prometheus counters, gauges, and latency histograms.
- Composite `/health` endpoint with sub-checks and HTTP status codes.
- Structured `SystemEvent` logging with TUI dashboard (ratatui).

**Operations & Deployment**
- Unified `AimpError` type hierarchy.
- Config validation (rejects invalid parameter combinations).
- Graceful shutdown with SIGINT/SIGTERM handling and 5-second timeout.
- Systemd hardened service file with security sandboxing.
- Firecracker microVM rootfs builder for multi-tenant edge gateways.
- Cross-compilation Makefile (ARM64, ARMv7, x86_64 musl static binaries).
- Cargo release profiles (LTO, strip, panic=abort).
- CI/CD: GitHub Actions for lint, test, security audit, docs, cross-compiled releases.

**Ecosystem**
- Python SDK (`aimp-client` package) with `AimpClient`, `AimpIdentity`, `OpCode`.
- CLI tool (`aimp-cli`) with `infer`, `ping`, `health`, `metrics` subcommands.
- Rustdoc published to GitHub Pages.
- TLA+ formal specification with convergence + quorum safety proofs.
- Chaos testing testbed (Python) for signature poisoning and replay attacks.
