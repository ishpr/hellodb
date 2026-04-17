use thiserror::Error;

#[derive(Debug, Error)]
pub enum EmbedError {
    #[error("HTTP {status}: {body}")]
    Http { status: u16, body: String },

    #[error("transport error: {0}")]
    Transport(String),

    #[error("auth error: check HELLODB_EMBED_*_TOKEN / API_KEY")]
    Auth,

    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("invalid response shape: {0}")]
    InvalidResponse(String),

    #[error("empty input")]
    EmptyInput,

    #[error("backend unavailable: {0}")]
    Unavailable(String),

    #[error("config error: {0}")]
    Config(String),

    #[cfg(feature = "fastembed")]
    #[error("fastembed error: {0}")]
    Fastembed(String),
}
