//! Error types for the sync layer.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum SyncError {
    #[error("backend error: {0}")]
    Backend(String),

    #[error("encryption error: {0}")]
    Encryption(String),

    #[error("decryption error: {0}")]
    Decryption(String),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("manifest not found for device '{device}' namespace '{namespace}'")]
    ManifestNotFound { device: String, namespace: String },

    #[error("storage error: {0}")]
    Storage(#[from] hellodb_storage::StorageError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("HTTP error {status}: {body}")]
    Http { status: u16, body: String },

    #[error("transport error: {0}")]
    Transport(String),

    #[error("authentication failed")]
    Auth,

    #[error("not found: {0}")]
    NotFound(String),
}
