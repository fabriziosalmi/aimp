use crate::protocol::envelope::{AimpData, AimpEnvelope, NodeId};
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

// ============================================================================
// Default backend: ed25519-dalek
// ============================================================================
#[cfg(not(feature = "fast-crypto"))]
mod backend {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
    use rand::rngs::OsRng;

    pub struct Identity {
        signing_key: SigningKey,
        pub verifying_key: VerifyingKey,
        pub noise_static_secret: x25519_dalek::StaticSecret,
        pub noise_static_public: x25519_dalek::PublicKey,
    }

    impl Identity {
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

        pub fn node_id(&self) -> NodeId {
            self.verifying_key.to_bytes()
        }

        pub fn sign(&self, data: AimpData) -> Result<AimpEnvelope, CryptoError> {
            let bytes =
                rmp_serde::to_vec(&data).map_err(|_| CryptoError::SerializationError)?;
            let signature = self.signing_key.sign(&bytes).to_bytes();
            Ok(AimpEnvelope { data, signature })
        }

        pub fn sign_bytes(&self, bytes: &[u8]) -> [u8; 64] {
            self.signing_key.sign(bytes).to_bytes()
        }
    }

    impl Default for Identity {
        fn default() -> Self {
            Self::new()
        }
    }
}

// ============================================================================
// Fast backend: ring (BoringSSL assembly)
// ============================================================================
#[cfg(feature = "fast-crypto")]
mod backend {
    use super::*;
    use ring::signature::KeyPair;

    pub struct Identity {
        key_pair: ring::signature::Ed25519KeyPair,
        public_key_bytes: [u8; 32],
        pub noise_static_secret: x25519_dalek::StaticSecret,
        pub noise_static_public: x25519_dalek::PublicKey,
    }

    impl Identity {
        pub fn new() -> Self {
            let rng = ring::rand::SystemRandom::new();
            let pkcs8 =
                ring::signature::Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
            let key_pair =
                ring::signature::Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();

            let mut public_key_bytes = [0u8; 32];
            public_key_bytes.copy_from_slice(key_pair.public_key().as_ref());

            // Derive X25519 keys from the first 32 bytes of PKCS8 seed
            // (ring doesn't expose raw secret, so we derive from pkcs8)
            let seed_bytes: [u8; 32] = {
                let mut b = [0u8; 32];
                // PKCS8 for Ed25519 has the seed at offset 16 (after ASN.1 header)
                b.copy_from_slice(&pkcs8.as_ref()[16..48]);
                b
            };
            let noise_static_secret = x25519_dalek::StaticSecret::from(seed_bytes);
            let noise_static_public =
                x25519_dalek::PublicKey::from(&noise_static_secret);

            Self {
                key_pair,
                public_key_bytes,
                noise_static_secret,
                noise_static_public,
            }
        }

        pub fn node_id(&self) -> NodeId {
            self.public_key_bytes
        }

        pub fn sign(&self, data: AimpData) -> Result<AimpEnvelope, CryptoError> {
            let bytes =
                rmp_serde::to_vec(&data).map_err(|_| CryptoError::SerializationError)?;
            let ring_sig = self.key_pair.sign(&bytes);
            let mut signature = [0u8; 64];
            signature.copy_from_slice(ring_sig.as_ref());
            Ok(AimpEnvelope { data, signature })
        }

        pub fn sign_bytes(&self, bytes: &[u8]) -> [u8; 64] {
            let ring_sig = self.key_pair.sign(bytes);
            let mut signature = [0u8; 64];
            signature.copy_from_slice(ring_sig.as_ref());
            signature
        }
    }

    impl Default for Identity {
        fn default() -> Self {
            Self::new()
        }
    }
}

pub use backend::Identity;
