use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("canonicalization failed: {0}")]
    Canonicalization(String),

    #[error("invalid record: {0}")]
    InvalidRecord(String),

    #[error("record payload too large: {size} bytes exceeds cap of {limit} bytes")]
    PayloadTooLarge { size: usize, limit: usize },

    #[error("schema not found: {0}")]
    SchemaNotFound(String),

    #[error("schema validation failed: {0}")]
    SchemaValidation(String),

    #[error("namespace not found: {0}")]
    NamespaceNotFound(String),

    #[error("branch not found: {0}")]
    BranchNotFound(String),

    #[error("branch is not active: {0}")]
    BranchNotActive(String),

    #[error("merge conflict: {0}")]
    MergeConflict(String),

    #[error("duplicate schema: {0}")]
    DuplicateSchema(String),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("crypto error: {0}")]
    Crypto(#[from] hellodb_crypto::CryptoError),
}
