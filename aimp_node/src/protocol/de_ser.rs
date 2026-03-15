use super::envelope::AimpEnvelope;
use thiserror::Error;

/// Strict Custom Error handling replacing `.unwrap()`
#[derive(Error, Debug)]
pub enum ParserError {
    #[error("MessagePack Encode Error: {0}")]
    EncodeFail(#[from] rmp_serde::encode::Error),
    #[error("MessagePack Decode Error: {0}")]
    DecodeFail(#[from] rmp_serde::decode::Error),
    #[error("Invalid Binary Size or Format")]
    InvalidSize,
}

/// Zero-copy oriented serialization abstraction.
pub struct ProtocolParser;

impl ProtocolParser {
    /// Compresses the Envelope into raw binary (MessagePack) for UDP payload.
    /// Strict determinism ensures identical output byte streams.
    pub fn to_bytes(envelope: &AimpEnvelope) -> Result<Vec<u8>, ParserError> {
        let bytes = rmp_serde::to_vec(envelope)?;
        Ok(bytes)
    }

    /// Decompresses the raw slice received from the network socket into the Rust Struct.
    /// This is the first level of parsing before the Crypto Firewall.
    pub fn from_bytes(bytes: &[u8]) -> Result<AimpEnvelope, ParserError> {
        if bytes.is_empty() {
            return Err(ParserError::InvalidSize);
        }
        let env: AimpEnvelope = rmp_serde::from_slice(bytes)?;
        
        // SOTA: Schema Evolution Guard
        if env.data.v != crate::config::PROTOCOL_VERSION {
            use serde::de::Error;
            return Err(ParserError::DecodeFail(rmp_serde::decode::Error::custom("Unsupported Protocol Version")));
        }
        
        Ok(env)
    }
}
