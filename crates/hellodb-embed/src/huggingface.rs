//! Hugging Face Inference API embeddings (`api-inference.huggingface.co`).

use serde_json::Value;

use crate::embed_toml::EmbedFile;
use crate::embedder::Embedder;
use crate::error::EmbedError;

const DEFAULT_HF_MODEL: &str = "sentence-transformers/all-MiniLM-L6-v2";
const DEFAULT_HF_DIM: usize = 384;

#[derive(Clone)]
pub struct HuggingFaceEmbedder {
    url: String,
    token: String,
    model: String,
    dim: usize,
    agent: ureq::Agent,
    timeout_ms: u64,
}

impl HuggingFaceEmbedder {
    pub fn new(url: impl Into<String>, token: impl Into<String>, model: String, dim: usize) -> Self {
        Self {
            url: url.into(),
            token: token.into(),
            model,
            dim,
            agent: ureq::AgentBuilder::new()
                .user_agent("hellodb-embed/0.1")
                .build(),
            timeout_ms: 60_000,
        }
    }

    /// Resolve token, model, dim from env and optional `embed.toml` (`huggingface` section).
    pub fn from_env_and_optional_file(file: Option<&EmbedFile>) -> Result<Self, EmbedError> {
        let sec = file.and_then(|f| f.huggingface.as_ref());
        let token = std::env::var("HELLODB_EMBED_HF_TOKEN")
            .or_else(|_| std::env::var("HF_TOKEN"))
            .ok()
            .or_else(|| sec.and_then(|s| s.token.clone()))
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                EmbedError::Config(
                    "Hugging Face token missing: set HELLODB_EMBED_HF_TOKEN or HF_TOKEN, \
                     or [huggingface].token in ~/.hellodb/embed.toml"
                        .into(),
                )
            })?;
        let model = std::env::var("HELLODB_EMBED_HF_MODEL")
            .ok()
            .or_else(|| sec.and_then(|s| s.model.clone()))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| DEFAULT_HF_MODEL.to_string());
        let dim = std::env::var("HELLODB_EMBED_HF_DIM")
            .ok()
            .and_then(|s| s.parse().ok())
            .or_else(|| sec.and_then(|s| s.dim))
            .unwrap_or(DEFAULT_HF_DIM);
        let base = std::env::var("HELLODB_EMBED_HF_URL").unwrap_or_else(|_| {
            "https://api-inference.huggingface.co/models".to_string()
        });
        let url = format!("{base}/{model}");
        Ok(Self::new(url, token, model, dim))
    }

    fn post_embed(&self, text: &str) -> Result<Value, EmbedError> {
        let body = serde_json::json!({ "inputs": text });
        let resp = self
            .agent
            .post(&self.url)
            .timeout(std::time::Duration::from_millis(self.timeout_ms))
            .set("Authorization", &format!("Bearer {}", self.token))
            .set("Content-Type", "application/json")
            .send_json(body);

        match resp {
            Ok(r) => {
                let status = r.status();
                if status == 401 || status == 403 {
                    return Err(EmbedError::Auth);
                }
                if !(200..300).contains(&status) {
                    return Err(EmbedError::Http {
                        status,
                        body: r.into_string().unwrap_or_default(),
                    });
                }
                r.into_json()
                    .map_err(|e| EmbedError::InvalidResponse(e.to_string()))
            }
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

fn value_to_embedding(v: &Value) -> Result<Vec<f32>, EmbedError> {
    if let Some(obj) = v.as_object() {
        if let Some(err) = obj.get("error").and_then(|e| e.as_str()) {
            return Err(EmbedError::Unavailable(err.to_string()));
        }
    }
    // Flat array of numbers
    if let Some(arr) = v.as_array() {
        if arr.first().and_then(|x| x.as_f64()).is_some() {
            return arr
                .iter()
                .map(|x| {
                    x.as_f64()
                        .map(|f| f as f32)
                        .ok_or_else(|| EmbedError::InvalidResponse("non-numeric embedding".into()))
                })
                .collect();
        }
        // Nested [[...]] — take first row
        if let Some(first) = arr.first() {
            return value_to_embedding(first);
        }
    }
    Err(EmbedError::InvalidResponse(
        "expected JSON array of floats or HF error object".into(),
    ))
}

impl Embedder for HuggingFaceEmbedder {
    fn embed_one(&self, text: &str) -> Result<Vec<f32>, EmbedError> {
        if text.is_empty() {
            return Err(EmbedError::EmptyInput);
        }
        let v = self.post_embed(text)?;
        let vec = value_to_embedding(&v)?;
        if vec.len() != self.dim {
            return Err(EmbedError::Config(format!(
                "embedding dim {} does not match HELLODB_EMBED_HF_DIM / configured dim {}",
                vec.len(),
                self.dim
            )));
        }
        Ok(vec)
    }

    fn dim(&self) -> usize {
        self.dim
    }

    fn model_id(&self) -> &str {
        &self.model
    }

    fn backend_name(&self) -> &'static str {
        "huggingface"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_flat_array() {
        let v = json!([0.1, 0.2, 0.3]);
        let out = value_to_embedding(&v).unwrap();
        assert_eq!(out.len(), 3);
    }

    #[test]
    fn parse_nested_array() {
        let v = json!([[0.5, -0.5]]);
        let out = value_to_embedding(&v).unwrap();
        assert_eq!(out, vec![0.5f32, -0.5f32]);
    }

    #[test]
    fn parse_hf_error() {
        let v = json!({"error": "model is loading"});
        assert!(value_to_embedding(&v).is_err());
    }
}
