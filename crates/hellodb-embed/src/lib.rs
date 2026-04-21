//! hellodb-embed — pluggable text → Vec<f32> layer for hellodb.
//!
//! Three backends ship out of the box:
//! - [`cloudflare::CloudflareGatewayEmbedder`]: routes to user's `hellodb-gateway`
//!   Worker which proxies Cloudflare Workers AI (~90K free embeddings/day).
//! - [`openai::OpenAICompatibleEmbedder`]: works with OpenAI, Voyage, Ollama,
//!   Together, vLLM — anything that speaks the standard embeddings API.
//! - [`huggingface::HuggingFaceEmbedder`]: Hugging Face Inference API.
//! - [`mock::MockEmbedder`]: deterministic hash-based, for tests and demos.
//!
//! Optional [`embed_toml::EmbedFile`] at `~/.hellodb/embed.toml` stores API keys
//! (mode `0600` on Unix). Environment variables override file values when both
//! are set.
//!
//! A fourth, [`fastembed_backend::FastembedLocal`], is feature-gated behind
//! `--features fastembed` and provides pure-Rust local inference at the cost
//! of ~40MB binary bloat.
//!
//! All backends implement [`Embedder`]. Select one at runtime via
//! [`build_from_env`].

pub mod cloudflare;
pub mod embed_toml;
pub mod embedder;
pub mod error;
pub mod huggingface;
pub mod mock;
pub mod openai;

#[cfg(feature = "fastembed")]
pub mod fastembed_backend;

pub use cloudflare::CloudflareGatewayEmbedder;
pub use embed_toml::{
    embed_config_path, remove_embed_file, try_load_embed_file, write_embed_file, EmbedFile,
    HuggingFaceFile, OpenAiFile,
};
pub use embedder::{build_from_env, Embedder};
pub use error::EmbedError;
pub use huggingface::HuggingFaceEmbedder;
pub use mock::MockEmbedder;
pub use openai::OpenAICompatibleEmbedder;
