//! hellodb-embed — pluggable text → Vec<f32> layer for hellodb.
//!
//! Three backends ship out of the box:
//! - [`cloudflare::CloudflareGatewayEmbedder`]: routes to user's `hellodb-gateway`
//!   Worker which proxies Cloudflare Workers AI (~90K free embeddings/day).
//! - [`openai::OpenAICompatibleEmbedder`]: works with OpenAI, Voyage, Ollama,
//!   Together, vLLM — anything that speaks the standard embeddings API.
//! - [`mock::MockEmbedder`]: deterministic hash-based, for tests and demos.
//!
//! A fourth, [`fastembed_backend::FastembedLocal`], is feature-gated behind
//! `--features fastembed` and provides pure-Rust local inference at the cost
//! of ~40MB binary bloat.
//!
//! All backends implement [`Embedder`]. Select one at runtime via
//! [`build_from_env`].

pub mod cloudflare;
pub mod embedder;
pub mod error;
pub mod mock;
pub mod openai;

#[cfg(feature = "fastembed")]
pub mod fastembed_backend;

pub use cloudflare::CloudflareGatewayEmbedder;
pub use embedder::{build_from_env, Embedder};
pub use error::EmbedError;
pub use mock::MockEmbedder;
pub use openai::OpenAICompatibleEmbedder;
