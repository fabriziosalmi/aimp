use super::envelope::AimpEnvelope;
use thiserror::Error;

/// Errors from MessagePack serialization/deserialization.
#[derive(Error, Debug)]
pub enum ParserError {
    #[error("MessagePack Encode Error: {0}")]
    EncodeFail(#[from] rmp_serde::encode::Error),
    #[error("MessagePack Decode Error: {0}")]
    DecodeFail(#[from] rmp_serde::decode::Error),
    #[error("Invalid Binary Size or Format")]
    InvalidSize,
}

/// Deterministic MessagePack serialization for the AIMP wire protocol.
///
/// Ensures identical byte output across all architectures by using
/// deterministic `rmp_serde` encoding with a protocol version guard.
pub struct ProtocolParser;

impl ProtocolParser {
    /// Serialize an envelope into MessagePack binary for UDP transmission.
    pub fn to_bytes(envelope: &AimpEnvelope) -> Result<Vec<u8>, ParserError> {
        let bytes = rmp_serde::to_vec(envelope)?;
        Ok(bytes)
    }

    /// Deserialize raw bytes into an envelope, with protocol version validation.
    pub fn from_bytes(bytes: &[u8]) -> Result<AimpEnvelope, ParserError> {
        if bytes.is_empty() {
            return Err(ParserError::InvalidSize);
        }
        let env: AimpEnvelope = rmp_serde::from_slice(bytes)?;

        // Accept current version and any version >= MIN_PROTOCOL_VERSION
        // to support rolling upgrades across the mesh.
        if env.data.v < crate::config::MIN_PROTOCOL_VERSION
            || env.data.v > crate::config::PROTOCOL_VERSION
        {
            use serde::de::Error;
            return Err(ParserError::DecodeFail(rmp_serde::decode::Error::custom(
                format!(
                    "Unsupported Protocol Version: {} (accepted: {}-{})",
                    env.data.v,
                    crate::config::MIN_PROTOCOL_VERSION,
                    crate::config::PROTOCOL_VERSION
                ),
            )));
        }

        Ok(env)
    }
}
