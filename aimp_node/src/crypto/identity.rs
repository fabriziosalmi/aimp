use ed25519_dalek::{SigningKey, VerifyingKey, Signer};
use rand::rngs::OsRng;
use thiserror::Error;
use crate::protocol::envelope::{AimpData, AimpEnvelope, NodeId};

#[derive(Error, Debug)]
pub enum CryptoError {
    #[error("Serialization failure during signing/verification")]
    SerializationError,
    #[error("Cryptographic verification failed: Data Poisoning detected")]
    InvalidSignature,
    #[error("Invalid NodeId: Could not derive Public Key")]
    InvalidPublicKey,
}

pub struct Identity {
    signing_key: SigningKey,
    pub verifying_key: VerifyingKey,
    // Noise static keys derived from Ed25519
    pub noise_static_secret: x25519_dalek::StaticSecret,
    pub noise_static_public: x25519_dalek::PublicKey,
}

impl Identity {
    /// Initialize identity using OS-grade entropy
    pub fn new() -> Self {
        let mut csprng = OsRng;
        let signing_key = SigningKey::generate(&mut csprng);
        let verifying_key = signing_key.verifying_key();
        
        // Derive X25519 from Ed25519 for Noise Protocol
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

    pub fn node_id(&self) -> NodeId {
        self.verifying_key.to_bytes()
    }

    /// Signs AimpData into a verified AimpEnvelope
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
