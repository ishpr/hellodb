//! Brain state persistence — where the tail cursor and run history live.
//!
//! We keep brain state out of hellodb deliberately: (a) records are
//! immutable content-addressed so "update cursor" would mean writing a new
//! record every time, (b) the brain needs to run even when hellodb's
//! SQLCipher handle is held by someone else. A simple JSON file is fine.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::BrainError;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct State {
    /// Highest monotonic seq observed across all completed passes. The next
    /// tail call uses this as `after_seq`.
    #[serde(default)]
    pub last_cursor: u64,
    /// Timestamp (ms) of the last *successful* pass that wrote a digest.
    /// Skipped passes (gates denied) do NOT update this.
    #[serde(default)]
    pub last_run_ms: u64,
    /// Monotonic count of successful digest passes.
    #[serde(default)]
    pub run_count: u64,
    /// For diagnostics: when the last pass was attempted, regardless of outcome.
    #[serde(default)]
    pub last_attempt_ms: u64,
    /// For diagnostics: the reason the last pass was skipped, if any.
    #[serde(default)]
    pub last_skip_reason: Option<String>,
}

impl State {
    pub fn load(path: &Path) -> Result<Self, BrainError> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let s = fs::read_to_string(path)
            .map_err(|e| BrainError::State(format!("reading {}: {e}", path.display())))?;
        serde_json::from_str(&s)
            .map_err(|e| BrainError::State(format!("parsing {}: {e}", path.display())))
    }

    pub fn save(&self, path: &Path) -> Result<(), BrainError> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let s = serde_json::to_string_pretty(self)?;
        // Atomic write: write to .tmp, rename.
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, s)?;
        fs::rename(&tmp, path)?;
        Ok(())
    }
}
