# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-03-15

### Added
- **Property-Based Testing**: Integrated `proptest` for Merkle-DAG verification.
- **Structured Logging**: Enums for system events (`SystemEvent`).
- **Resiliency Layer**: Backpressure (Semaphore) and Circuit Breaker (DashMap) for networking.
- **Architectural Records**: ADRs for Merkle-CRDT and Epoch GC.
- **CLI Interface**: Structured argument parsing via `Clap`.

### Changed
- **Rebranding**: Reverted version to `v0.1.x` and removed marketing-oriented language.
- **Networking**: Replaced unbounded sets with bounded LRU ring buffers.
- **Epoch GC**: Optimized historical pruning to maintain constant memory footprint.

### Fixed
- **Port Conflict**: Graceful handling of "Address in use" errors at startup.
- **Data Parity**: Fixed serialization mismatches between Rust and Python bridge.

## [0.1.0] - 2026-03-15

### Added
- Initial implementation of the Merkle-CRDT synchronization engine.
- AI Feedback loop for deterministic decentralized inference.
- TUI Dashboard for network monitoring.
