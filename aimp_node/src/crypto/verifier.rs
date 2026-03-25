use crate::protocol::{envelope::AimpEnvelope, Hash32};
use blake3;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};

/// Zero-trust security firewall for AIMP envelope verification and hashing.
///
/// All incoming packets must pass through `verify()` before any internal processing.
/// Uses Ed25519 for signature verification and BLAKE3 for Merkle-DAG hashing.
pub struct SecurityFirewall;

impl SecurityFirewall {
    /// Verify the Ed25519 signature on an envelope.
    ///
    /// Recovers the public key from the envelope's `origin_pubkey` field,
    /// re-serializes the data deterministically, and checks the signature.
    /// Returns `false` on any failure (invalid key, bad signature, serialization error).
    pub fn verify(envelope: &AimpEnvelope) -> bool {
        let pubkey = match VerifyingKey::from_bytes(&envelope.data.origin_pubkey) {
            Ok(key) => key,
            Err(_) => return false,
        };

        let signature = Signature::from_bytes(&envelope.signature);

        let bytes = match rmp_serde::to_vec(&envelope.data) {
            Ok(b) => b,
            Err(_) => return false,
        };

        pubkey.verify(&bytes, &signature).is_ok()
    }

    /// Compute a BLAKE3 hash of the given data, returning a 32-byte digest.
    pub fn hash(data: &[u8]) -> Hash32 {
        let mut hasher = blake3::Hasher::new();
        hasher.update(data);
        *hasher.finalize().as_bytes()
    }

    /// Compute BLAKE3 hashes for multiple payloads (SIMD-accelerated internally).
    pub fn batch_hash(payloads: &[Vec<u8>]) -> Vec<Hash32> {
        payloads.iter().map(|p| Self::hash(p)).collect()
    }
}
