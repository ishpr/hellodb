//! The `Embedder` trait — what every embedding backend implements.
//!
//! Design: this is the only surface hellodb-brain and hellodb-mcp need to
//! know about. Backends register themselves via [`build_from_env`] which
//! picks one based on `HELLODB_EMBED_BACKEND`. That keeps the wiring shallow
//! and means adding a new backend is one match arm.

use crate::error::EmbedError;

/// Uniform embedding interface. Impls must be cheap to clone/share; use
/// connection pooling inside (ureq::Agent already does this).
pub trait Embedder: Send + Sync {
    /// Embed a single text. Returns a dense vector. Must match `dim()`.
    fn embed_one(&self, text: &str) -> Result<Vec<f32>, EmbedError>;

    /// Batch-embed. Default impl falls back to serial `embed_one` — backends
    /// that natively support batching (Cloudflare, OpenAI-compatible) override
    /// this for a meaningful cost saving.
    fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbedError> {
        texts.iter().map(|t| self.embed_one(t)).collect()
    }

    /// Dimensionality of every vector this embedder returns. Used to validate
    /// against the target vector index at upsert time.
    fn dim(&self) -> usize;

    /// Human-readable model id (e.g. `"@cf/baai/bge-small-en-v1.5"`). Stored
    /// alongside embeddings so you can detect model drift later.
    fn model_id(&self) -> &str;

    /// Short backend name for diagnostics (`"cloudflare_gateway"`, `"openai"`,
    /// `"mock"`, `"fastembed"`).
    fn backend_name(&self) -> &'static str;
}

/// Environment-driven factory. Reads `HELLODB_EMBED_BACKEND` and constructs
/// the appropriate [`Embedder`]. Other variables are backend-specific — see
/// each backend's `from_env()` for the exact list.
///
/// `HELLODB_EMBED_BACKEND` values:
/// - `"cloudflare"` — hits the user's own gateway Worker at `HELLODB_EMBED_GATEWAY_URL`
/// - `"openai"` — hits any OpenAI-compatible embeddings endpoint (OpenAI, Voyage, Ollama, vLLM, Together)
/// - `"huggingface"` / `"hf"` — Hugging Face Inference API (`HELLODB_EMBED_HF_TOKEN` or `[huggingface]` in `embed.toml`)
/// - `"mock"` — deterministic, for tests
/// - `"fastembed"` — pure-Rust local (requires `fastembed` feature)
/// - `""` or unset — if `~/.hellodb/embed.toml` sets `backend`, that value is used; else error
pub fn build_from_env() -> Result<Box<dyn Embedder>, EmbedError> {
    let file = crate::embed_toml::try_load_embed_file();
    let backend = std::env::var("HELLODB_EMBED_BACKEND")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| file.as_ref().and_then(|f| f.backend.clone()))
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            EmbedError::Config(
                "HELLODB_EMBED_BACKEND unset and no backend in ~/.hellodb/embed.toml. \
                 set HELLODB_EMBED_BACKEND (e.g. in your MCP host's env UI) or add embed.toml."
                    .into(),
            )
        })?;
    match backend.as_str() {
        "cloudflare" | "cloudflare_gateway" => {
            Ok(Box::new(crate::cloudflare::CloudflareGatewayEmbedder::from_env()?))
        }
        "openai" | "openai_compatible" => Ok(Box::new(
            crate::openai::OpenAICompatibleEmbedder::from_env_with_optional_file(file.as_ref())?,
        )),
        "huggingface" | "hf" => Ok(Box::new(crate::huggingface::HuggingFaceEmbedder::from_env_and_optional_file(
            file.as_ref(),
        )?)),
        "mock" => Ok(Box::new(crate::mock::MockEmbedder::default())),
        #[cfg(feature = "fastembed")]
        "fastembed" | "local" => Ok(Box::new(crate::fastembed_backend::FastembedLocal::from_env()?)),
        other => Err(EmbedError::Config(format!(
            "unknown HELLODB_EMBED_BACKEND '{other}'. valid: cloudflare, openai, huggingface, mock, fastembed (feature-gated)"
        ))),
    }
}
