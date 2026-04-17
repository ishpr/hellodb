use thiserror::Error;

#[derive(Debug, Error)]
pub enum BrainError {
    #[error("config error: {0}")]
    Config(String),

    #[error("state error: {0}")]
    State(String),

    #[error("lock error: {0}")]
    Lock(String),

    #[error("identity error: {0}")]
    Identity(String),

    #[error("storage error: {0}")]
    Storage(#[from] hellodb_storage::StorageError),

    #[error("core error: {0}")]
    Core(#[from] hellodb_core::CoreError),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}
