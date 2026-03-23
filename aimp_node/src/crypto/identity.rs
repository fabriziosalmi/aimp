use crate::protocol::envelope::{AimpData, AimpEnvelope, NodeId};
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use rand::rngs::OsRng;
use thiserror::Error;

/// Errors that can occur during cryptographic operations.
#[derive(Error, Debug)]
pub enum CryptoError {
    #[error("Serialization failure during signing/verification")]
    SerializationError,
    #[error("Cryptographic verification failed: Data Poisoning detected")]
    InvalidSignature,
    #[error("Invalid NodeId: Could not derive Public Key")]
    InvalidPublicKey,
}

/// Cryptographic identity for an AIMP mesh node.
///
/// Holds an Ed25519 signing keypair for message authentication and
/// X25519 static keys derived from Ed25519 for Noise Protocol sessions.
pub struct Identity {
    signing_key: SigningKey,
    /// The Ed25519 verifying (public) key.
    pub verifying_key: VerifyingKey,
    /// X25519 static secret for Noise Protocol handshakes.
    pub noise_static_secret: x25519_dalek::StaticSecret,
    /// X25519 static public key for Noise Protocol.
    pub noise_static_public: x25519_dalek::PublicKey,
}

impl Identity {
    /// Generate a new random identity using OS-grade entropy.
    pub fn new() -> Self {
        let mut csprng = OsRng;
        let signing_key = SigningKey::generate(&mut csprng);
        let verifying_key = signing_key.verifying_key();

        let secret_bytes = signing_key.to_bytes();
        let noise_static_secret = x25519_dalek::StaticSecret::from(secret_bytes);
        let noise_static_public = x25519_dalek::PublicKey::from(&noise_static_secret);

        Self {
            signing_key,
            verifying_key,
            noise_static_secret,
            noise_static_public,
        }
    }

    /// Return the 32-byte node identifier (Ed25519 public key).
    pub fn node_id(&self) -> NodeId {
        self.verifying_key.to_bytes()
    }

    /// Sign an `AimpData` payload, producing a verified `AimpEnvelope`.
    pub fn sign(&self, data: AimpData) -> Result<AimpEnvelope, CryptoError> {
        let bytes = rmp_serde::to_vec(&data).map_err(|_| CryptoError::SerializationError)?;
        let signature = self.signing_key.sign(&bytes).to_bytes();
        Ok(AimpEnvelope { data, signature })
    }
}

impl Default for Identity {
    fn default() -> Self {
        Self::new()
    }
}
