use ed25519_dalek::{VerifyingKey, Signature, Verifier};
use crate::protocol::{envelope::AimpEnvelope, Hash32};
use blake3;

pub struct SecurityFirewall;

impl SecurityFirewall {
    /// Hot-path verification engine.
    /// Mathematically validates the integrity of the envelope BEFORE any internal processing.
    pub fn verify(envelope: &AimpEnvelope) -> bool {
        // 1. Recover public key from origin
        let pubkey = match VerifyingKey::from_bytes(&envelope.data.origin_pubkey) {
            Ok(key) => key,
            Err(_) => return false,
        };

        // 2. Wrap signature bytes (Zero-alloc)
        let signature = Signature::from_bytes(&envelope.signature);

        // 3. Deterministic serialization for cross-check
        // WARNING: Cross-architecture SLM hashing needs BTreeMap in VectorClock
        let bytes = match rmp_serde::to_vec(&envelope.data) {
            Ok(b) => b,
            Err(_) => return false,
        };

        // 4. Crypto-verification
        pubkey.verify(&bytes, &signature).is_ok()
    }

    /// Fast Blake3 hash for Merkle-DAG nodes
    pub fn hash(data: &[u8]) -> Hash32 {
        let mut hasher = blake3::Hasher::new();
        hasher.update(data);
        *hasher.finalize().as_bytes()
    }

    /// SIMD-accelerated batch hashing for multiple payloads.
    pub fn batch_hash(payloads: &[Vec<u8>]) -> Vec<Hash32> {
        // blake3 uses SIMD internally. In a massive mesh, this is the bottleneck.
        payloads.iter().map(|p| Self::hash(p)).collect()
    }
}
