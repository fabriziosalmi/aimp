use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};
use std::collections::BTreeMap;

/// 32-byte Ed25519 public key used as a universally unique node identifier.
pub type NodeId = [u8; 32];

/// 32-byte BLAKE3 cryptographic hash used throughout the Merkle-DAG.
pub type Hash32 = [u8; 32];

/// Protocol operation codes as defined in SPEC.md.
#[derive(Debug, Clone, Copy, Serialize_repr, Deserialize_repr, PartialEq, Eq)]
#[repr(u8)]
pub enum OpCode {
    /// Gossip broadcast containing the current Merkle root.
    Ping = 0x01,
    /// Request a delta sync from a peer.
    SyncReq = 0x02,
    /// Response containing delta nodes for synchronization.
    SyncRes = 0x03,
    /// BFT quorum AI inference request.
    Infer = 0x04,
}

/// Deterministic data payload for the AIMP wire protocol.
///
/// All fields are serialized via MessagePack with deterministic ordering
/// (BTreeMap for vclock) to ensure identical byte streams across architectures.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AimpData {
    /// Protocol version (currently 0x01).
    pub v: u8,
    /// Operation code identifying the message type.
    pub op: OpCode,
    /// Time-to-live hop counter; decremented on each broadcast.
    pub ttl: u8,
    /// Ed25519 public key of the originating node.
    #[serde(with = "serde_bytes")]
    pub origin_pubkey: NodeId,
    /// Vector clock for causal ordering (node_id_prefix -> tick).
    pub vclock: BTreeMap<String, u64>,
    /// Raw payload bytes. Use `Payload::decode` for type-safe access per opcode.
    #[serde(with = "serde_bytes")]
    pub payload: Vec<u8>,
}

/// Type-safe payload interpretation per opcode.
///
/// The wire format remains `Vec<u8>` for backward compatibility.
/// Use `Payload::decode` to get the typed variant, and `Payload::encode`
/// to produce bytes for inclusion in `AimpData`.
#[derive(Debug, Clone)]
pub enum Payload {
    /// Ping: contains the sender's current Merkle root hash.
    PingRoot(Hash32),
    /// SyncReq: list of the sender's current head hashes.
    SyncRequest(Vec<Hash32>),
    /// SyncRes: list of DAG nodes the sender is sharing.
    SyncResponse(Vec<crate::crdt::merkle_dag::DagNode>),
    /// Infer: a UTF-8 prompt string for deterministic AI inference.
    InferPrompt(String),
    /// Fallback for unknown or malformed payloads.
    Raw(Vec<u8>),
}

impl Payload {
    /// Decode raw payload bytes based on the opcode.
    pub fn decode(op: OpCode, bytes: &[u8]) -> Self {
        match op {
            OpCode::Ping => {
                if bytes.len() == 32 {
                    let mut h = [0u8; 32];
                    h.copy_from_slice(bytes);
                    Payload::PingRoot(h)
                } else {
                    Payload::Raw(bytes.to_vec())
                }
            }
            OpCode::SyncReq => rmp_serde::from_slice::<Vec<Hash32>>(bytes)
                .map(Payload::SyncRequest)
                .unwrap_or_else(|_| Payload::Raw(bytes.to_vec())),
            OpCode::SyncRes => {
                rmp_serde::from_slice::<Vec<crate::crdt::merkle_dag::DagNode>>(bytes)
                    .map(Payload::SyncResponse)
                    .unwrap_or_else(|_| Payload::Raw(bytes.to_vec()))
            }
            OpCode::Infer => String::from_utf8(bytes.to_vec())
                .map(Payload::InferPrompt)
                .unwrap_or_else(|_| Payload::Raw(bytes.to_vec())),
        }
    }

    /// Encode a typed payload into raw bytes for wire transmission.
    pub fn encode(&self) -> Vec<u8> {
        match self {
            Payload::PingRoot(h) => h.to_vec(),
            Payload::SyncRequest(heads) => rmp_serde::to_vec(heads).unwrap_or_default(),
            Payload::SyncResponse(nodes) => rmp_serde::to_vec(nodes).unwrap_or_default(),
            Payload::InferPrompt(s) => s.as_bytes().to_vec(),
            Payload::Raw(b) => b.clone(),
        }
    }
}

/// Zero-trust cryptographic envelope wrapping an `AimpData` payload
/// with a fixed-size Ed25519 signature for non-repudiation.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AimpEnvelope {
    /// The signed data payload.
    pub data: AimpData,
    /// 64-byte Ed25519 signature over the MessagePack-serialized `data`.
    #[serde(with = "serde_bytes")]
    pub signature: [u8; 64],
}
