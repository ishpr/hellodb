//! Fact extraction from episodes — the "digestion" step.
//!
//! MVP ships a `mock` backend: deterministic, LLM-free, good enough to
//! prove the pipeline mechanics end-to-end. Plug a real LLM backend in
//! by implementing [`DigestBackend`] and wiring it in the main.rs match
//! against `config.digest.backend`.
//!
//! Intentional design: digestion is a pluggable trait, not a baked-in
//! HTTP client. The brain orchestrates; the LLM call (or its absence)
//! is strictly a plug-in. This keeps the brain crate free of network
//! dependencies and makes the whole pipeline testable without a running
//! model server.

use hellodb_storage::TailEntry;
use serde::{Deserialize, Serialize};

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
}

pub trait DigestBackend {
    fn digest(&self, episodes: &[TailEntry], config: &Config) -> Result<Vec<Fact>, BrainError>;
}

/// Pick a backend based on config. MVP: only "mock" is wired.
/// Plug in "openai" / "anthropic" / "local-http" here when ready.
pub fn select_backend(name: &str) -> Result<Box<dyn DigestBackend>, BrainError> {
    match name {
        "mock" => Ok(Box::new(MockBackend)),
        other => Err(BrainError::Config(format!(
            "unknown digest backend '{other}'. MVP supports: mock"
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
                        // Trim long text to 120 chars for readability
                        if s.len() > 120 {
                            format!("{}…", &s[..120])
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
                }
            })
            .collect();

        Ok(facts)
    }
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
}
