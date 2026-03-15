use serde::{Deserialize, Serialize};

use serde_repr::{Deserialize_repr, Serialize_repr};
use std::collections::BTreeMap;

// ==========================================
// CORE DATA TYPES (AIMP v0.1.0)
// ==========================================

/// Universally unique Node Identifier (32-byte Ed25519 Public Key)
pub type NodeId = [u8; 32];

/// A 32-byte cryptographic hash (Blake3)
pub type Hash32 = [u8; 32];

/// Opcodes defined in SPEC.md
#[derive(Debug, Clone, Copy, Serialize_repr, Deserialize_repr, PartialEq, Eq)]
#[repr(u8)]
pub enum OpCode {
    Ping      = 0x01, // Gossip broadcast containing Merkle Root
    SyncReq   = 0x02, // Delta sync request
    SyncRes   = 0x03, // Delta sync response
    Infer     = 0x04, // BFT Quorum AI calculation
}

/// Deterministic Data Payload
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AimpData {
    pub v: u8,               // Protocol Version (0x01)
    pub op: OpCode,          // Primitive OpCode
    pub ttl: u8,             // Hop decay
    #[serde(with = "serde_bytes")]
    pub origin_pubkey: NodeId, 
    pub vclock: BTreeMap<String, u64>, // Direct map for deterministic serialization (Transparent)
    #[serde(with = "serde_bytes")]
    pub payload: Vec<u8>,    
}

/// Zero-Trust Cryptographic Wrapper
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AimpEnvelope {
    pub data: AimpData,      
    #[serde(with = "serde_bytes")]
    pub signature: [u8; 64], // Fixed-size Ed25519 signature (Zero-alloc)
}
