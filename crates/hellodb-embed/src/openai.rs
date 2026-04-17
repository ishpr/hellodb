//! OpenAI-compatible embeddings backend.
//!
//! This shape is the de-facto lingua franca — works against:
//! - OpenAI (`https://api.openai.com/v1`)
//! - Voyage AI (`https://api.voyageai.com/v1`)
//! - Together.ai, Groq, Anyscale, self-hosted vLLM
//! - Ollama (`http://localhost:11434/v1` with `embeddings` endpoint)
//!
//! We deliberately don't bake in OpenAI-specific quirks; the user points
//! at whatever endpoint they have credentials for.

use serde::{Deserialize, Serialize};

use crate::embedder::Embedder;
use crate::error::EmbedError;

#[derive(Clone)]
pub struct OpenAICompatibleEmbedder {
    endpoint: String, // e.g. "https://api.openai.com/v1/embeddings"
    api_key: String,
    model: String,
    dim: usize,
    agent: ureq::Agent,
    timeout_ms: u64,
}

impl OpenAICompatibleEmbedder {
    pub fn new(
        endpoint: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
        dim: usize,
    ) -> Self {
        Self {
            endpoint: endpoint.into(),
            api_key: api_key.into(),
            model: model.into(),
            dim,
            agent: ureq::AgentBuilder::new()
                .user_agent("hellodb-embed/0.1")
                .build(),
            timeout_ms: 30_000,
        }
    }

    pub fn with_timeout(mut self, ms: u64) -> Self {
        self.timeout_ms = ms;
        self
    }

    /// Build from env:
    ///   HELLODB_EMBED_OPENAI_ENDPOINT  — full URL to the embeddings endpoint
    ///   HELLODB_EMBED_OPENAI_KEY       — API key
    ///   HELLODB_EMBED_OPENAI_MODEL     — model name (e.g. "text-embedding-3-small")
    ///   HELLODB_EMBED_OPENAI_DIM       — output dim (so we don't have to call once to probe)
    ///
    /// Falls back to OPENAI_API_KEY if HELLODB_EMBED_OPENAI_KEY is unset.
    pub fn from_env() -> Result<Self, EmbedError> {
        let endpoint = std::env::var("HELLODB_EMBED_OPENAI_ENDPOINT").map_err(|_| {
            EmbedError::Config(
                "HELLODB_EMBED_OPENAI_ENDPOINT unset (e.g. https://api.openai.com/v1/embeddings)"
                    .into(),
            )
        })?;
        let api_key = std::env::var("HELLODB_EMBED_OPENAI_KEY")
            .or_else(|_| std::env::var("OPENAI_API_KEY"))
            .map_err(|_| {
                EmbedError::Config("HELLODB_EMBED_OPENAI_KEY (or OPENAI_API_KEY) unset".into())
            })?;
        let model = std::env::var("HELLODB_EMBED_OPENAI_MODEL")
            .unwrap_or_else(|_| "text-embedding-3-small".into());
        let dim = std::env::var("HELLODB_EMBED_OPENAI_DIM")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(1536); // default for text-embedding-3-small
        Ok(Self::new(endpoint, api_key, model, dim))
    }

    fn request(&self, input: EmbedInput<'_>) -> Result<OpenAIEmbeddingsResponse, EmbedError> {
        let body = OpenAIEmbeddingsRequest {
            input,
            model: &self.model,
        };
        let resp = self
            .agent
            .post(&self.endpoint)
            .timeout(std::time::Duration::from_millis(self.timeout_ms))
            .set("Authorization", &format!("Bearer {}", self.api_key))
            .set("Content-Type", "application/json")
            .send_json(serde_json::to_value(&body)?);

        match resp {
            Ok(r) => r
                .into_json()
                .map_err(|e| EmbedError::InvalidResponse(e.to_string())),
            Err(ureq::Error::Status(401, _)) | Err(ureq::Error::Status(403, _)) => {
                Err(EmbedError::Auth)
            }
            Err(ureq::Error::Status(code, r)) => Err(EmbedError::Http {
                status: code,
                body: r.into_string().unwrap_or_default(),
            }),
            Err(ureq::Error::Transport(t)) => Err(EmbedError::Transport(t.to_string())),
        }
    }
}

#[derive(Serialize)]
struct OpenAIEmbeddingsRequest<'a> {
    input: EmbedInput<'a>,
    model: &'a str,
}

#[derive(Serialize)]
#[serde(untagged)]
enum EmbedInput<'a> {
    Single(&'a str),
    Batch(&'a [String]),
}

#[derive(Deserialize)]
struct OpenAIEmbeddingsResponse {
    data: Vec<EmbeddingDatum>,
}

#[derive(Deserialize)]
struct EmbeddingDatum {
    embedding: Vec<f32>,
    #[serde(default)]
    #[allow(dead_code)]
    index: usize,
}

impl Embedder for OpenAICompatibleEmbedder {
    fn embed_one(&self, text: &str) -> Result<Vec<f32>, EmbedError> {
        if text.is_empty() {
            return Err(EmbedError::EmptyInput);
        }
        let resp = self.request(EmbedInput::Single(text))?;
        resp.data
            .into_iter()
            .next()
            .map(|d| d.embedding)
            .ok_or_else(|| EmbedError::InvalidResponse("empty data[]".into()))
    }

    fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbedError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let resp = self.request(EmbedInput::Batch(texts))?;
        Ok(resp.data.into_iter().map(|d| d.embedding).collect())
    }

    fn dim(&self) -> usize {
        self.dim
    }
    fn model_id(&self) -> &str {
        &self.model
    }
    fn backend_name(&self) -> &'static str {
        "openai_compatible"
    }
}
