//! Cloudflare Workers AI embedding backend — routed through the user's
//! own `hellodb-gateway` Worker.
//!
//! We intentionally do NOT talk to `api.cloudflare.com` directly. The
//! gateway Worker owns the Workers AI binding and the R2 bucket; local
//! hellodb only holds the gateway URL + bearer token. This gives the
//! user one revocable credential and a single edge-enforced rate limiter.

use serde::{Deserialize, Serialize};

use crate::embedder::Embedder;
use crate::error::EmbedError;

/// Default model — 384-dim, fast, uses ~0.11 neurons per call so the free
/// 10K/day tier covers ~90K embedding calls/day.
pub const DEFAULT_MODEL: &str = "@cf/baai/bge-small-en-v1.5";

fn dim_for_model(model: &str) -> usize {
    match model {
        "@cf/baai/bge-small-en-v1.5" => 384,
        "@cf/baai/bge-base-en-v1.5" => 768,
        "@cf/baai/bge-large-en-v1.5" => 1024,
        _ => 384, // caller should set model explicitly if using something else
    }
}

/// Optional Cloudflare Access service-token credentials. If set, we send
/// `CF-Access-Client-Id` + `CF-Access-Client-Secret` on every request so
/// the Worker can sit behind Cloudflare Access without breaking headless
/// tools like hellodb. See gateway/README.md for how to enable Access and
/// create a service token.
#[derive(Clone, Default)]
pub struct AccessServiceToken {
    pub client_id: String,
    pub client_secret: String,
}

#[derive(Clone)]
pub struct CloudflareGatewayEmbedder {
    gateway_url: String,
    token: String,
    access: Option<AccessServiceToken>,
    model: String,
    dim: usize,
    agent: ureq::Agent,
    timeout_ms: u64,
}

impl CloudflareGatewayEmbedder {
    pub fn new(
        gateway_url: impl Into<String>,
        token: impl Into<String>,
        model: Option<String>,
    ) -> Self {
        let model = model.unwrap_or_else(|| DEFAULT_MODEL.to_string());
        let dim = dim_for_model(&model);
        Self {
            gateway_url: gateway_url.into().trim_end_matches('/').to_string(),
            token: token.into(),
            access: None,
            model,
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

    /// Attach Cloudflare Access service-token credentials. Use this when the
    /// Worker is behind CF Access — without these headers, Access will
    /// redirect the request to a browser login page and the ureq call will
    /// get an HTML response instead of JSON.
    pub fn with_access_service_token(
        mut self,
        id: impl Into<String>,
        secret: impl Into<String>,
    ) -> Self {
        self.access = Some(AccessServiceToken {
            client_id: id.into(),
            client_secret: secret.into(),
        });
        self
    }

    pub fn from_env() -> Result<Self, EmbedError> {
        let url = std::env::var("HELLODB_EMBED_GATEWAY_URL").map_err(|_| {
            EmbedError::Config(
                "HELLODB_EMBED_GATEWAY_URL unset (e.g. https://ish.workers.dev)".into(),
            )
        })?;
        let token = std::env::var("HELLODB_EMBED_GATEWAY_TOKEN")
            .map_err(|_| EmbedError::Config("HELLODB_EMBED_GATEWAY_TOKEN unset".into()))?;
        let model = std::env::var("HELLODB_EMBED_MODEL").ok();
        let mut embedder = Self::new(url, token, model);

        // Optional Access service-token env vars. Both must be set to activate.
        if let (Ok(id), Ok(secret)) = (
            std::env::var("HELLODB_EMBED_CF_ACCESS_CLIENT_ID"),
            std::env::var("HELLODB_EMBED_CF_ACCESS_CLIENT_SECRET"),
        ) {
            embedder = embedder.with_access_service_token(id, secret);
        }
        Ok(embedder)
    }

    fn request(&self, body: &EmbedRequest<'_>) -> Result<EmbedResponse, EmbedError> {
        let url = format!("{}/embed", self.gateway_url);
        let mut req = self
            .agent
            .post(&url)
            .timeout(std::time::Duration::from_millis(self.timeout_ms))
            .set("Authorization", &format!("Bearer {}", self.token))
            .set("Content-Type", "application/json");
        if let Some(ref a) = self.access {
            req = req
                .set("CF-Access-Client-Id", &a.client_id)
                .set("CF-Access-Client-Secret", &a.client_secret);
        }
        let resp = req.send_json(serde_json::to_value(body)?);

        match resp {
            Ok(r) => {
                let v: EmbedResponse = r
                    .into_json()
                    .map_err(|e| EmbedError::InvalidResponse(e.to_string()))?;
                Ok(v)
            }
            Err(ureq::Error::Status(401, _)) | Err(ureq::Error::Status(403, _)) => {
                Err(EmbedError::Auth)
            }
            Err(ureq::Error::Status(code, r)) => {
                let body = r.into_string().unwrap_or_default();
                Err(EmbedError::Http { status: code, body })
            }
            Err(ureq::Error::Transport(t)) => Err(EmbedError::Transport(t.to_string())),
        }
    }
}

#[derive(Serialize)]
#[serde(untagged)]
enum EmbedRequest<'a> {
    Single { text: &'a str, model: &'a str },
    Batch { texts: &'a [String], model: &'a str },
}

#[derive(Deserialize)]
#[serde(untagged)]
enum EmbedResponse {
    Single {
        embedding: Vec<f32>,
        #[serde(default)]
        #[allow(dead_code)]
        dim: Option<usize>,
        #[serde(default)]
        #[allow(dead_code)]
        model: Option<String>,
    },
    Batch {
        embeddings: Vec<Vec<f32>>,
        #[serde(default)]
        #[allow(dead_code)]
        dim: Option<usize>,
        #[serde(default)]
        #[allow(dead_code)]
        model: Option<String>,
    },
}

impl Embedder for CloudflareGatewayEmbedder {
    fn embed_one(&self, text: &str) -> Result<Vec<f32>, EmbedError> {
        if text.is_empty() {
            return Err(EmbedError::EmptyInput);
        }
        let body = EmbedRequest::Single {
            text,
            model: &self.model,
        };
        let resp = self.request(&body)?;
        match resp {
            EmbedResponse::Single { embedding, .. } => Ok(embedding),
            EmbedResponse::Batch { embeddings, .. } => embeddings
                .into_iter()
                .next()
                .ok_or_else(|| EmbedError::InvalidResponse("empty batch".into())),
        }
    }

    fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbedError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let body = EmbedRequest::Batch {
            texts,
            model: &self.model,
        };
        let resp = self.request(&body)?;
        match resp {
            EmbedResponse::Batch { embeddings, .. } => Ok(embeddings),
            EmbedResponse::Single { embedding, .. } => Ok(vec![embedding]),
        }
    }

    fn dim(&self) -> usize {
        self.dim
    }
    fn model_id(&self) -> &str {
        &self.model
    }
    fn backend_name(&self) -> &'static str {
        "cloudflare_gateway"
    }
}
