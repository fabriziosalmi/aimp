# AIMP v0.1.0 Protocol Specification

## 1. Abstract
The AI Mesh Protocol (AIMP) is a decentralized communication framework for deterministic state synchronization between autonomous agents in high-entropy, low-bandwidth, and uncertain network environments.

## 2. Mathematical Foundation
AIMP v2 operates on the principle of a **Convergent Merkle-DAG**. 

### 2.1 The State Lattice
State $S$ is defined as a Join-Semilattice $(L, \sqcup)$ where:
- $\forall a, b \in L, a \sqcup b \in L$ (Closure)
- $a \sqcup b = b \sqcup a$ (Commutativity)
- $(a \sqcup b) \sqcup c = a \sqcup (b \sqcup c)$ (Associativity)
- $a \sqcup a = a$ (Idempotency)

### 2.2 Merkle Integrity
Every mutation $m$ is identified by $H(m)$, where $H$ is the BLAKE3 cryptographic hash.
$H(m) = BLAKE3(Payload \ || \ Causality \ || \ Origin)$

## 3. Wire Protocol
Binary data is transmitted over UDP via the **AimpEnvelope**.

| Offset (Bytes) | Field | Description |
|---|---|---|
| 0 | Version | 0x02 (AIMP) |
| 1 | OpCode | 0x01-0x04 (PING, SYNC_REQ, SYNC_RES, INFER) |
| 2 | TTL | 8-bit Time-to-Live |
| 3-34 | Origin | 32-byte Ed25519 Public Key |
| 35-X | Payload | MessagePack encoded deterministically |
| X-END | Signature | 64-byte Ed25519 Signature |

## 4. Consensus (Edge-BFT)
A request for `OP_INFER` requires $K$ independent nodes (Byzantine Fault Tolerance, BFT) to publish an identical hash $H(Result)$ within the same Epoch $\epsilon$ to be considered valid state in the Merkle-DAG.

## 5. Glossary
- **DAG**: Directed Acyclic Graph.
- **CRDT**: Conflict-free Replicated Data Type.
- **BFT**: Byzantine Fault Tolerance.
- **GC**: Garbage Collection.
- **TTL**: Time-to-Live.
- **MessagePack**: A binary serialization format.
- **Epoch Garbage Collection**: A mechanism to prune old DAG nodes based on causal cycles.
