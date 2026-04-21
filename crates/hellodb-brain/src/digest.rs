//! Fact extraction from episodes — the "digestion" step.
//!
//! `mock` backend: deterministic, LLM-free, good enough for offline tests.
//! `openrouter` backend: production path that calls OpenRouter chat completions
//! and parses a strict JSON schema for consolidated facts.

use hellodb_storage::TailEntry;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::config::Config;
use crate::error::BrainError;

/// One consolidated fact produced by the digest step.
///
/// The `derived_from` field preserves lineage: every fact can be traced back
/// to the raw episodes that produced it. Essential for the contradiction-
/// resolution story (a later pass can see what evidence a fact is built on
/// and decide whether new episodes corroborate or contradict it).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fact {
    /// One-sentence canonical statement of the fact.
    pub statement: String,
    /// Short topic tag (e.g. "workflow", "preferences", "codebase").
    pub topic: String,
    /// Confidence in [0.0, 1.0]. The mock backend uses a simple heuristic;
    /// a real LLM backend returns its own score.
    pub confidence: f32,
    /// Record ids of the episodes this fact was derived from.
    pub derived_from: Vec<String>,
    /// Free-form rationale the backend wants to attach.
    #[serde(default)]
    pub rationale: Option<String>,
    /// If Some, this fact is meant to replace an existing fact with the
    /// given record_id. Archival of the old record is an inherently
    /// human-in-the-loop decision, so facts with a Some here are ALWAYS
    /// held for review, regardless of `confidence` or auto-merge threshold.
    ///
    /// The MockBackend never sets this. Real LLM backends set it when
    /// they detect that a new episode contradicts an existing fact.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supersedes: Option<String>,
}

pub trait DigestBackend {
    fn digest(&self, episodes: &[TailEntry], config: &Config) -> Result<Vec<Fact>, BrainError>;
}

const OPENROUTER_DEFAULT_BASE: &str = "https://openrouter.ai/api/v1";
const OPENROUTER_DEFAULT_MODEL: &str = "openai/gpt-4o-mini";

/// Pick a backend based on config.
pub fn select_backend(name: &str) -> Result<Box<dyn DigestBackend>, BrainError> {
    match name {
        "mock" => Ok(Box::new(MockBackend)),
        "openrouter" => Ok(Box::new(OpenRouterBackend::from_env()?)),
        other => Err(BrainError::Config(format!(
            "unknown digest backend '{other}'. supported: mock, openrouter"
        ))),
    }
}

/// Heuristic, LLM-free backend. Groups episodes by the `topic` field of their
/// data if present, else by `schema`. Emits one fact per group summarizing
/// the member episodes. Deterministic — good for tests and demos.
pub struct MockBackend;

impl DigestBackend for MockBackend {
    fn digest(&self, episodes: &[TailEntry], _config: &Config) -> Result<Vec<Fact>, BrainError> {
        use std::collections::BTreeMap;
        let mut groups: BTreeMap<String, Vec<&TailEntry>> = BTreeMap::new();

        for entry in episodes {
            let topic = entry
                .record
                .data
                .get("topic")
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .unwrap_or_else(|| entry.record.schema.clone());
            groups.entry(topic).or_default().push(entry);
        }

        let facts = groups
            .into_iter()
            .map(|(topic, members)| {
                // Crude "statement" = the first member's text, trimmed; or a
                // generic description if no text field.
                let statement = members
                    .first()
                    .and_then(|m| {
                        m.record
                            .data
                            .get("text")
                            .or_else(|| m.record.data.get("rule"))
                    })
                    .and_then(|v| v.as_str())
                    .map(|s| {
                        // Trim long text to 120 chars for readability. `s.len()` is BYTES,
                        // and `&s[..120]` panics if byte 120 lands inside a multi-byte
                        // UTF-8 boundary (common with non-ASCII episode text). Truncate
                        // on character boundaries instead.
                        let char_count = s.chars().count();
                        if char_count > 120 {
                            let head: String = s.chars().take(120).collect();
                            format!("{head}…")
                        } else {
                            s.to_string()
                        }
                    })
                    .unwrap_or_else(|| {
                        format!("{} episodes observed on topic '{topic}'", members.len())
                    });

                let derived_from: Vec<String> =
                    members.iter().map(|m| m.record.record_id.clone()).collect();

                // Confidence scales with support: 1 episode -> 0.5, ramping to 0.95 at 10.
                let confidence = 0.5 + (members.len().min(10) as f32 / 10.0) * 0.45;

                Fact {
                    statement,
                    topic,
                    confidence,
                    derived_from,
                    rationale: Some(format!("mock: grouped {} episodes by topic", members.len())),
                    supersedes: None,
                }
            })
            .collect();

        Ok(facts)
    }
}

struct OpenRouterBackend {
    api_key: String,
    model: String,
    base_url: String,
    fallback_to_mock: bool,
}

impl OpenRouterBackend {
    fn from_env() -> Result<Self, BrainError> {
        let api_key = std::env::var("HELLODB_BRAIN_OPENROUTER_API_KEY")
            .map_err(|_| {
                BrainError::Config(
                    "openrouter backend selected but HELLODB_BRAIN_OPENROUTER_API_KEY is missing"
                        .into(),
                )
            })?
            .trim()
            .to_string();
        if api_key.is_empty() {
            return Err(BrainError::Config(
                "HELLODB_BRAIN_OPENROUTER_API_KEY is empty".into(),
            ));
        }

        let model = std::env::var("HELLODB_BRAIN_OPENROUTER_MODEL")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| OPENROUTER_DEFAULT_MODEL.into());
        let base_url = std::env::var("HELLODB_BRAIN_OPENROUTER_BASE_URL")
            .ok()
            .map(|s| s.trim().trim_end_matches('/').to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| OPENROUTER_DEFAULT_BASE.into());
        let fallback_to_mock = std::env::var("HELLODB_BRAIN_OPENROUTER_FALLBACK_TO_MOCK")
            .map(|v| matches!(v.trim(), "1" | "true" | "TRUE" | "yes" | "YES"))
            .unwrap_or(false);

        Ok(Self {
            api_key,
            model,
            base_url,
            fallback_to_mock,
        })
    }

    fn call_openrouter(
        &self,
        episodes: &[TailEntry],
        config: &Config,
    ) -> Result<Vec<Fact>, BrainError> {
        let endpoint = format!("{}/chat/completions", self.base_url);
        let prompt = build_digest_prompt(episodes, config);
        let payload = json!({
            "model": self.model,
            "response_format": { "type": "json_object" },
            "messages": [
                {
                    "role": "system",
                    "content": "You convert user session episodes into durable memory facts. Return strict JSON with shape: {\"facts\":[{statement:string,topic:string,confidence:number,derived_from:string[],rationale?:string,supersedes?:string}]}. Confidence MUST be in [0,1]. Keep facts concise and avoid duplicates."
                },
                {
                    "role": "user",
                    "content": prompt
                }
            ]
        });

        let response = ureq::post(&endpoint)
            .set("Authorization", &format!("Bearer {}", self.api_key))
            .set("Content-Type", "application/json")
            .send_json(payload)
            .map_err(|e| BrainError::State(format!("openrouter request failed: {e}")))?;

        let body: Value = response
            .into_json()
            .map_err(|e| BrainError::State(format!("openrouter response parse failed: {e}")))?;
        let content = body
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .and_then(|v| v.get("message"))
            .and_then(|v| v.get("content"))
            .and_then(Value::as_str)
            .ok_or_else(|| {
                BrainError::State("openrouter response missing message content".into())
            })?;

        parse_facts_payload(content)
    }
}

impl DigestBackend for OpenRouterBackend {
    fn digest(&self, episodes: &[TailEntry], config: &Config) -> Result<Vec<Fact>, BrainError> {
        match self.call_openrouter(episodes, config) {
            Ok(facts) => Ok(facts),
            Err(e) => {
                if self.fallback_to_mock {
                    eprintln!("brain: openrouter digest failed, falling back to mock backend: {e}");
                    MockBackend.digest(episodes, config)
                } else {
                    Err(e)
                }
            }
        }
    }
}

/// Serialise tailed episodes into a digest prompt, with per-episode and
/// per-prompt size caps applied.
///
/// The digest step runs outside the agent turn, but the LLM driving it still
/// has a context window and still pays the "reasoning vs self-filtering"
/// tax that context-stuffing imposes on any model. We therefore:
///   1. Truncate each episode's `data` field to `max_episode_chars` UTF-8
///      characters (head, not tail — the intent usually lives up front),
///   2. Stop adding episodes once the prompt reaches `max_prompt_chars`.
///
/// Dropped episodes are safe to leave behind: the brain's tail cursor only
/// advances on successful digest, so they reappear in the next pass.
fn build_digest_prompt(episodes: &[TailEntry], config: &Config) -> String {
    let max_episode_chars = config.limits.max_episode_chars;
    let max_prompt_chars = config.limits.max_prompt_chars;

    let mut rows: Vec<Value> = Vec::with_capacity(episodes.len());
    let mut running_chars: usize = 0;
    let mut dropped: usize = 0;

    for e in episodes {
        let truncated_data = truncate_json_value(&e.record.data, max_episode_chars);
        let row = json!({
            "seq": e.seq,
            "record_id": e.record.record_id,
            "schema": e.record.schema,
            "created_at_ms": e.record.created_at_ms,
            "data": truncated_data,
        });

        // Cheap size probe — compact JSON length is the practical proxy for
        // how much the row will cost in the final pretty-printed prompt.
        let row_chars = serde_json::to_string(&row)
            .map(|s| s.chars().count())
            .unwrap_or(0);

        if running_chars + row_chars > max_prompt_chars && !rows.is_empty() {
            dropped = episodes.len() - rows.len();
            break;
        }

        running_chars += row_chars;
        rows.push(row);
    }

    let body = serde_json::to_string_pretty(&rows).unwrap_or_else(|_| "[]".into());
    let suffix = if dropped > 0 {
        format!(
            "\n\n(note: {dropped} episode(s) deferred to the next pass to stay under the prompt size cap)"
        )
    } else {
        String::new()
    };

    format!(
        "Episodes (JSON, per-field truncated):\n{body}{suffix}\n\nExtract durable facts only. Avoid transient chatter. Use derived_from record_ids from the input.",
    )
}

/// Truncate a JSON value so no single string leaf exceeds `max_chars` UTF-8
/// characters. Objects and arrays are recursed into; non-string leaves pass
/// through unchanged. Truncation respects character boundaries (never cuts
/// inside a multi-byte codepoint) and appends a single `…` marker.
fn truncate_json_value(v: &Value, max_chars: usize) -> Value {
    match v {
        Value::String(s) => Value::String(truncate_string_chars(s, max_chars)),
        Value::Array(a) => Value::Array(a.iter().map(|x| truncate_json_value(x, max_chars)).collect()),
        Value::Object(m) => Value::Object(
            m.iter()
                .map(|(k, val)| (k.clone(), truncate_json_value(val, max_chars)))
                .collect(),
        ),
        other => other.clone(),
    }
}

fn truncate_string_chars(s: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let mut end_byte = s.len();
    for (count, (i, _)) in s.char_indices().enumerate() {
        if count == max_chars {
            end_byte = i;
            break;
        }
    }
    if end_byte < s.len() {
        let mut out = String::with_capacity(end_byte + 3);
        out.push_str(&s[..end_byte]);
        out.push('…');
        out
    } else {
        s.to_string()
    }
}

fn parse_facts_payload(raw: &str) -> Result<Vec<Fact>, BrainError> {
    let parsed: Value = serde_json::from_str(raw)
        .map_err(|e| BrainError::State(format!("digest backend returned invalid JSON: {e}")))?;

    let candidate = if parsed.is_array() {
        parsed
    } else if let Some(facts) = parsed.get("facts") {
        facts.clone()
    } else {
        return Err(BrainError::State(
            "digest backend JSON must be an array or object with `facts`".into(),
        ));
    };

    let mut facts: Vec<Fact> = serde_json::from_value(candidate).map_err(|e| {
        BrainError::State(format!("digest backend JSON had invalid fact shape: {e}"))
    })?;
    if facts.is_empty() {
        return Err(BrainError::State(
            "digest backend returned an empty fact list".into(),
        ));
    }

    for fact in &mut facts {
        if fact.statement.trim().is_empty() {
            return Err(BrainError::State(
                "digest backend returned a fact with empty statement".into(),
            ));
        }
        if fact.topic.trim().is_empty() {
            return Err(BrainError::State(
                "digest backend returned a fact with empty topic".into(),
            ));
        }
        if !fact.confidence.is_finite() {
            return Err(BrainError::State(
                "digest backend returned non-finite confidence".into(),
            ));
        }
        fact.confidence = fact.confidence.clamp(0.0, 1.0);
        if fact.derived_from.is_empty() {
            return Err(BrainError::State(
                "digest backend returned a fact with empty derived_from".into(),
            ));
        }
    }
    Ok(facts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hellodb_core::Record;
    use hellodb_crypto::KeyPair;
    use hellodb_storage::TailEntry;
    use serde_json::json;

    fn ep(topic: &str, text: &str, seq: u64) -> TailEntry {
        let kp = KeyPair::generate();
        let record = Record::new_with_timestamp(
            &kp.signing,
            "test.episode".into(),
            "test".into(),
            json!({ "topic": topic, "text": text }),
            None,
            1000,
        )
        .unwrap();
        TailEntry {
            seq,
            branch: "test/main".into(),
            record,
        }
    }

    #[test]
    fn groups_by_topic() {
        let cfg = Config::with_defaults(std::path::Path::new("/tmp"));
        let backend = MockBackend;
        let episodes = vec![
            ep("workflow", "use pnpm", 1),
            ep("workflow", "tabs not spaces", 2),
            ep("codebase", "rust workspace", 3),
        ];
        let facts = backend.digest(&episodes, &cfg).unwrap();
        assert_eq!(facts.len(), 2);
        // BTreeMap iteration is sorted: codebase, workflow
        assert_eq!(facts[0].topic, "codebase");
        assert_eq!(facts[1].topic, "workflow");
        assert_eq!(facts[1].derived_from.len(), 2);
    }

    #[test]
    fn confidence_scales_with_support() {
        let cfg = Config::with_defaults(std::path::Path::new("/tmp"));
        let backend = MockBackend;
        let episodes = vec![ep("x", "a", 1)];
        let facts = backend.digest(&episodes, &cfg).unwrap();
        assert!((facts[0].confidence - 0.545).abs() < 0.01);

        let episodes: Vec<_> = (0..10).map(|i| ep("x", "a", i)).collect();
        let facts = backend.digest(&episodes, &cfg).unwrap();
        assert!((facts[0].confidence - 0.95).abs() < 0.01);
    }

    #[test]
    fn parse_facts_payload_rejects_invalid_json() {
        let err = parse_facts_payload("{not json").unwrap_err();
        assert!(err.to_string().contains("invalid JSON"));
    }

    #[test]
    fn parse_facts_payload_rejects_empty_facts() {
        let err = parse_facts_payload(r#"{"facts":[]}"#).unwrap_err();
        assert!(err.to_string().contains("empty fact list"));
    }

    #[test]
    fn parse_facts_payload_accepts_supersedes() {
        let facts = parse_facts_payload(
            r#"{
              "facts": [
                {
                  "statement": "Use pnpm in this repo",
                  "topic": "workflow",
                  "confidence": 0.92,
                  "derived_from": ["r1","r2"],
                  "rationale": "seen in multiple sessions",
                  "supersedes": "old-fact-id"
                }
              ]
            }"#,
        )
        .unwrap();
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].supersedes.as_deref(), Some("old-fact-id"));
        assert!((facts[0].confidence - 0.92).abs() < 0.001);
    }

    #[test]
    fn build_digest_prompt_truncates_oversize_fields() {
        let cfg = Config::with_defaults(std::path::Path::new("/tmp"));
        let long = "x".repeat(10_000);
        let episodes = vec![ep("topic", &long, 1)];
        let prompt = build_digest_prompt(&episodes, &cfg);
        // Default per-episode cap is 2000 chars; the prompt must not carry
        // the full 10k body, but must carry the truncation marker.
        assert!(prompt.chars().count() < 5_000, "prompt should be truncated");
        assert!(prompt.contains('…'), "prompt should carry truncation marker");
        assert!(!prompt.contains(&"x".repeat(3_000)));
    }

    #[test]
    fn build_digest_prompt_respects_prompt_size_ceiling() {
        // Very tight per-episode and per-prompt caps so a small batch trips
        // the overall prompt ceiling and later episodes get deferred.
        let mut cfg = Config::with_defaults(std::path::Path::new("/tmp"));
        cfg.limits.max_episode_chars = 100;
        cfg.limits.max_prompt_chars = 300;

        let episodes: Vec<_> = (0..20)
            .map(|i| ep("topic", &format!("entry {i} body"), i as u64))
            .collect();
        let prompt = build_digest_prompt(&episodes, &cfg);

        // At least one episode must land (first episode always goes in,
        // otherwise the pass would stall indefinitely) and the prompt
        // must carry the deferred-episodes note.
        assert!(prompt.contains("deferred to the next pass"));
        assert!(prompt.contains("entry 0 body"));
        // And it must NOT contain the tail — prove back-pressure actually
        // dropped episodes rather than silently stuffing them.
        assert!(!prompt.contains("entry 19 body"));
    }

    #[test]
    fn truncate_string_chars_is_utf8_safe() {
        // 'é' is a 2-byte codepoint. A byte-based truncation at 3 would
        // panic on char boundary; we truncate on char boundary instead.
        let s = "abéde";
        let out = truncate_string_chars(s, 3);
        assert_eq!(out, "abé…");
    }

    #[test]
    fn openrouter_backend_requires_api_key() {
        let key = std::env::var("HELLODB_BRAIN_OPENROUTER_API_KEY").ok();
        std::env::remove_var("HELLODB_BRAIN_OPENROUTER_API_KEY");
        let res = OpenRouterBackend::from_env();
        if let Some(v) = key {
            std::env::set_var("HELLODB_BRAIN_OPENROUTER_API_KEY", v);
        }
        assert!(res.is_err());
    }
}
