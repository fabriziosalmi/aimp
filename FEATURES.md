# AIMP (AI Mesh Protocol) - Feature Matrix

This document lists the verified engineering features implemented in the AIMP protocol.

## Backend (Mesh Daemon - Rust `aimp_node`)

- **UDP Gossip Networking**: Non-blocking epidemic propagation implemented with `tokio` for mesh resilience.
- **Cryptographic Verification (Ed25519)**: Zero-trust packet validation. Invalid signatures result in immediate drop and security logging.
- **State Synchronization (Merkle-CRDT)**: Merkle-DAG causality tracking and Vector Clocks for eventual consistency in fragmented topologies.
- **Deterministic Inference**: Heuristic bridge for local, fixed-parameter AI logic to ensure global response parity.
- **MessagePack Compression**: Binary-perfect serialization for low-bandwidth environments.
- **Backpressure & Resiliency**: Semaphore-based throughput limiting and circuit breakers for peer health management.

## Frontend (Testbed & Chaos Tooling - Python `aimp_testbed`)

- **Identity Management**: Local Ed25519 keypair generation for sensor identity.
- **Protocol Parity**: 1:1 MessagePack implementation mirroring the Rust parser for bidirectional communication.
- **Chaos & Poisoning Simulation**: Automated security audit tools for testing malleability, signature failure, and replay protection.
- **Task Injection (`OP_INFER`)**: Standardized broadcast mechanism for injecting AI inference requests into the mesh.
