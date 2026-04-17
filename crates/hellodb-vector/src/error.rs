use thiserror::Error;

#[derive(Debug, Error)]
pub enum VectorError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("crypto error: {0}")]
    Crypto(#[from] hellodb_crypto::CryptoError),

    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("dimension mismatch: expected {expected}, got {got}")]
    DimensionMismatch { expected: usize, got: usize },

    #[error("invalid embedding: {0}")]
    InvalidEmbedding(String),
}
