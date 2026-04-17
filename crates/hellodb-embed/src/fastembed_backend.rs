//! Pure-Rust local embedder via `fastembed-rs`.
//!
//! Feature-gated: enable with `--features fastembed`. This pulls in ONNX
//! runtime (~40MB binary bloat) and downloads model weights to the fastembed
//! cache on first use (`~/.cache/fastembed/` by default). Off by default so
//! the standard build stays lean; on for users who want zero network
//! dependence.

#![cfg(feature = "fastembed")]

use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};

use crate::embedder::Embedder;
use crate::error::EmbedError;

pub struct FastembedLocal {
    model: EmbeddingModel,
    inner: TextEmbedding,
    dim: usize,
}

fn dim_for(model: &EmbeddingModel) -> usize {
    match model {
        EmbeddingModel::BGESmallENV15 => 384,
        EmbeddingModel::BGEBaseENV15 => 768,
        EmbeddingModel::BGELargeENV15 => 1024,
        EmbeddingModel::AllMiniLML6V2 => 384,
        _ => 384,
    }
}

fn model_id(model: &EmbeddingModel) -> &'static str {
    match model {
        EmbeddingModel::BGESmallENV15 => "BAAI/bge-small-en-v1.5",
        EmbeddingModel::BGEBaseENV15 => "BAAI/bge-base-en-v1.5",
        EmbeddingModel::BGELargeENV15 => "BAAI/bge-large-en-v1.5",
        EmbeddingModel::AllMiniLML6V2 => "sentence-transformers/all-MiniLM-L6-v2",
        _ => "fastembed-unknown",
    }
}

impl FastembedLocal {
    pub fn new(model: EmbeddingModel) -> Result<Self, EmbedError> {
        let inner = TextEmbedding::try_new(InitOptions {
            model_name: model.clone(),
            show_download_progress: false,
            ..Default::default()
        })
        .map_err(|e| EmbedError::Fastembed(e.to_string()))?;
        let dim = dim_for(&model);
        Ok(Self { model, inner, dim })
    }

    pub fn from_env() -> Result<Self, EmbedError> {
        // HELLODB_EMBED_FASTEMBED_MODEL: "bge-small" (default) | "bge-base" | "bge-large" | "minilm-l6"
        let name =
            std::env::var("HELLODB_EMBED_FASTEMBED_MODEL").unwrap_or_else(|_| "bge-small".into());
        let model = match name.as_str() {
            "bge-small" => EmbeddingModel::BGESmallENV15,
            "bge-base" => EmbeddingModel::BGEBaseENV15,
            "bge-large" => EmbeddingModel::BGELargeENV15,
            "minilm-l6" => EmbeddingModel::AllMiniLML6V2,
            other => {
                return Err(EmbedError::Config(format!(
                    "unknown fastembed model '{other}'. valid: bge-small, bge-base, bge-large, minilm-l6"
                )));
            }
        };
        Self::new(model)
    }
}

impl Embedder for FastembedLocal {
    fn embed_one(&self, text: &str) -> Result<Vec<f32>, EmbedError> {
        if text.is_empty() {
            return Err(EmbedError::EmptyInput);
        }
        let mut out = self
            .inner
            .embed(vec![text.to_string()], None)
            .map_err(|e| EmbedError::Fastembed(e.to_string()))?;
        out.pop()
            .ok_or_else(|| EmbedError::Fastembed("empty result".into()))
    }

    fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbedError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        self.inner
            .embed(texts.to_vec(), None)
            .map_err(|e| EmbedError::Fastembed(e.to_string()))
    }

    fn dim(&self) -> usize {
        self.dim
    }
    fn model_id(&self) -> &str {
        model_id(&self.model)
    }
    fn backend_name(&self) -> &'static str {
        "fastembed"
    }
}
