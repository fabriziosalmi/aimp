use thiserror::Error;

/// Unified error type for all AIMP node operations.
///
/// Replaces `Box<dyn Error>` throughout the codebase, enabling
/// structured error matching and better diagnostics.
#[derive(Error, Debug)]
pub enum AimpError {
    /// Cryptographic operation failed (signing, verification, key derivation).
    #[error("Crypto error: {0}")]
    Crypto(#[from] crate::crypto::CryptoError),

    /// MessagePack serialization or deserialization failed.
    #[error("Protocol error: {0}")]
    Protocol(#[from] crate::protocol::de_ser::ParserError),

    /// Persistent storage (redb) operation failed.
    #[error("Storage error: {0}")]
    Storage(String),

    /// Network I/O error.
    #[error("Network error: {0}")]
    Network(#[from] std::io::Error),

    /// Configuration loading or validation error.
    #[error("Config error: {0}")]
    Config(String),

    /// AEAD encryption or decryption failed.
    #[error("Encryption error: {0}")]
    Encryption(String),

    /// AI inference engine error.
    #[error("Inference error: {0}")]
    Inference(String),
}

/// Convenience alias used throughout the codebase.
pub type AimpResult<T> = Result<T, AimpError>;

// Bridge from redb errors
impl From<redb::Error> for AimpError {
    fn from(e: redb::Error) -> Self {
        AimpError::Storage(e.to_string())
    }
}

impl From<redb::DatabaseError> for AimpError {
    fn from(e: redb::DatabaseError) -> Self {
        AimpError::Storage(e.to_string())
    }
}

impl From<redb::TableError> for AimpError {
    fn from(e: redb::TableError) -> Self {
        AimpError::Storage(e.to_string())
    }
}

impl From<redb::TransactionError> for AimpError {
    fn from(e: redb::TransactionError) -> Self {
        AimpError::Storage(e.to_string())
    }
}

impl From<redb::StorageError> for AimpError {
    fn from(e: redb::StorageError) -> Self {
        AimpError::Storage(e.to_string())
    }
}

impl From<redb::CommitError> for AimpError {
    fn from(e: redb::CommitError) -> Self {
        AimpError::Storage(e.to_string())
    }
}

impl From<rmp_serde::encode::Error> for AimpError {
    fn from(e: rmp_serde::encode::Error) -> Self {
        AimpError::Protocol(crate::protocol::de_ser::ParserError::EncodeFail(e))
    }
}

impl From<rmp_serde::decode::Error> for AimpError {
    fn from(e: rmp_serde::decode::Error) -> Self {
        AimpError::Protocol(crate::protocol::de_ser::ParserError::DecodeFail(e))
    }
}
