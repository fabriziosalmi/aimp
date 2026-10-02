# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.5.0] - 2026-10-02

### Changed — BREAKING

- **L3 epistemic layer: `correlation_cell: None` is no longer summed at full weight.**
  Until now, an absent correlation cell meant "uncorrelated, therefore independent".
  That equated *unverified* independence with *established* independence, and since
  declaring a cell can only ever reduce a source's weight, withholding it was strictly
  advantageous: measured, concealment paid **70.3x** over honest disclosure.

  Aggregation is now two-level. Independence that can be checked — a distinct embedding
  cluster — still sums at full weight, so genuinely independent evidence raises confidence
  without bound. Independence that is merely asserted is grouped by the best available
  fallback and discounted against other unassessed groups.

  **Migration:** deployments that never set `correlation_cell` and carry no
  `QuantizedEmbedding` will see aggregate confidence fall. This is intended. To recover
  full weight for genuinely independent sources, attach embeddings; correlated sources
  should declare a shared cell.

### Added

- `LogOdds::aggregate_hierarchical` — two-level correlation-aware aggregation.
- `LogOdds::MAX_DECLARED` / `MIN_DECLARED` and `Claim::declared_confidence()` — bound
  self-declared confidence, which was previously unvalidated on the wire.
- `correlation_groups()` — derives correlation groups from exact Hamming clustering
  rather than trusting the declared cell.
- `KnowledgeGraph::propagate_trust_correlated` — correlation discounting on the trust
  propagation path, which the v0.3.0 defense never reached.
- `Identity::from_secret_bytes` / `Identity::secret_bytes` — round-trip the Ed25519 seed
  so an embedder keeps a stable node id across restarts.
- `cli` feature (on by default). The terminal dashboard, argument parsing, HTTP metrics
  endpoint and config loader now sit behind it: library consumers using
  `default-features = false` build 100 crates instead of 192.

### Fixed

- Correlation discounting was absent from `propagate_trust_advanced` and
  `LogOddsBeliefEngine::compute`; 100 correlated sources amplified a conclusion to
  30,908 where the reducer capped the same 100 at 723.
- A self-declared `embedding_version` exempted a claim from the correlation check:
  declaring one version per identity restored the whole pre-fix attack (50,800 vs an
  honest 723). The version is now in no decision key; cluster ids change accordingly
  (domain `aimp.correlation.cluster.v2`) (#21).
- `--all-features` did not compile: the `fast-crypto` (ring) `Identity` backend lacked the
  seed round-trip. Both backends are now pinned to an RFC 8032 vector and CI tests
  `--all-features` (#23).
- Self-declared `confidence` was unbounded (`i32::MAX` accepted from the wire).
- Embeddings were not bound to claim content, allowing forged distinct embeddings to
  manufacture independence.
- `LogOdds::to_percent` overstated every positive confidence by one bracket
  (`from_percent(60) = 405` but `to_percent(405)` returned 70). The negative side was
  correctly aligned, so the error was systematic and one-directional.

### Security

- `prometheus` no longer pulls `protobuf` 2.28 (RUSTSEC-2024-0437): only the text
  exposition is used, so its default features are off (#22).
- Dependency bumps: `ring` 0.17.12, `rand` 0.8.6, `msgpack` 1.2.1, `pynacl` 1.6.2.

### Known limitations

- Embedding determinism is now **safety-critical**: two honest builds producing different
  embeddings for identical content flag each other as inconsistent and both degrade to
  unassessed. Since #21 this holds whatever `embedding_version` they declare, because the
  version no longer partitions the check. Fail-closed, but a new failure mode.
- K independently-delegated identities with distinct evidence sources and no embeddings
  remain indistinguishable from K independent sources by aggregation alone. Tracked in
  issue #10. Closing it needs authorization bound into the epoch certificate (issue #8),
  planned for a later release.

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

**Decision Engine & Consensus**
- Pluggable `DecisionEngine` trait with `RuleEngine` implementation.
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
