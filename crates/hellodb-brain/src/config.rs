//! TOML config schema for the brain.
//!
//! A passive-memory pipeline is only as good as its gates. This config
//! controls what namespaces to watch, where to write digests, and how
//! aggressive the firing thresholds are.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::BrainError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub data: DataConfig,
    pub namespaces: NamespacesConfig,
    pub gates: GatesConfig,
    #[serde(default)]
    pub limits: LimitsConfig,
    #[serde(default)]
    pub digest: DigestConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataConfig {
    /// Path to hellodb's SQLite database (same file hellodb-mcp opens).
    pub db_path: PathBuf,
    /// Path to the identity.key file (same as hellodb-mcp).
    pub identity_path: PathBuf,
    /// Where the brain persists its own cursor/run state. JSON file.
    pub state_path: PathBuf,
    /// File lock path — brain refuses to run if this file exists.
    pub lock_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamespacesConfig {
    /// Namespace the brain tails for raw episodes (session turns, events).
    pub episodes: String,
    /// Namespace where consolidated facts land. Drafts go to
    /// `{facts}/digest-{timestamp}` branches; user merges them to main.
    pub facts: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatesConfig {
    /// Minimum wall-clock time that must elapse since the last successful
    /// run before the brain will fire again. Think "cool-down period."
    pub min_time_since_last_run_ms: u64,
    /// Minimum number of new episodes observed since the last run.
    /// Without enough new material there's nothing worth digesting.
    pub min_episodes_since_last_run: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LimitsConfig {
    /// Hard cap on episodes consumed per pass. Prevents a burst of activity
    /// from triggering a huge LLM call.
    pub max_episodes_per_pass: usize,
}

impl Default for LimitsConfig {
    fn default() -> Self {
        Self {
            max_episodes_per_pass: 200,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DigestConfig {
    /// Which digest backend to use. MVP ships "mock" — an LLM-free heuristic
    /// that proves the pipeline end-to-end. Real LLM backends (openai,
    /// anthropic, local-http) are future work.
    #[serde(default = "default_backend")]
    pub backend: String,
    /// Schema id used when writing consolidated facts. Must live in the
    /// facts namespace. Auto-registered on first use.
    #[serde(default = "default_fact_schema")]
    pub fact_schema_id: String,
}

impl Default for DigestConfig {
    fn default() -> Self {
        Self {
            backend: default_backend(),
            fact_schema_id: default_fact_schema(),
        }
    }
}

fn default_backend() -> String {
    "mock".into()
}
fn default_fact_schema() -> String {
    "brain.fact".into()
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, BrainError> {
        let s = std::fs::read_to_string(path)
            .map_err(|e| BrainError::Config(format!("reading {}: {e}", path.display())))?;
        toml::from_str(&s)
            .map_err(|e| BrainError::Config(format!("parsing {}: {e}", path.display())))
    }

    /// Sensible defaults for a fresh install. Writes to $HELLODB_HOME/brain.toml
    /// on first run if no config file exists.
    pub fn with_defaults(data_dir: &Path) -> Self {
        Self {
            data: DataConfig {
                db_path: data_dir.join("local.db"),
                identity_path: data_dir.join("identity.key"),
                state_path: data_dir.join("brain.state.json"),
                lock_path: data_dir.join("brain.lock"),
            },
            namespaces: NamespacesConfig {
                episodes: "claude.episodes".into(),
                facts: "claude.facts".into(),
            },
            gates: GatesConfig {
                // 6 hours — long enough to batch a session's worth of material
                min_time_since_last_run_ms: 6 * 60 * 60 * 1000,
                min_episodes_since_last_run: 5,
            },
            limits: LimitsConfig::default(),
            digest: DigestConfig::default(),
        }
    }
}
