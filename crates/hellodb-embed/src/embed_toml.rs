//! Optional `~/.hellodb/embed.toml` for embedding provider credentials.
//! Environment variables take precedence over file values when both are set.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::EmbedError;

/// Resolved path: `$HELLODB_HOME/embed.toml` unless `HELLODB_EMBED_CONFIG` is set.
pub fn embed_config_path() -> PathBuf {
    if let Ok(p) = std::env::var("HELLODB_EMBED_CONFIG") {
        return PathBuf::from(p);
    }
    hellodb_home().join("embed.toml")
}

fn hellodb_home() -> PathBuf {
    if let Ok(h) = std::env::var("HELLODB_HOME") {
        return PathBuf::from(h);
    }
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".hellodb")
}

#[derive(Debug, Default, Deserialize, Serialize, Clone)]
pub struct EmbedFile {
    /// When set, used if `HELLODB_EMBED_BACKEND` is unset or empty.
    #[serde(default)]
    pub backend: Option<String>,
    #[serde(default)]
    pub openai: Option<OpenAiFile>,
    #[serde(default)]
    pub huggingface: Option<HuggingFaceFile>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct OpenAiFile {
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub endpoint: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub dim: Option<usize>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct HuggingFaceFile {
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub dim: Option<usize>,
}

pub fn load_embed_file() -> Option<EmbedFile> {
    let path = embed_config_path();
    let raw = std::fs::read_to_string(&path).ok()?;
    toml::from_str(&raw).ok()
}

pub fn write_embed_file(file: &EmbedFile) -> Result<(), EmbedError> {
    let path = embed_config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| EmbedError::Config(e.to_string()))?;
    }
    let s = toml::to_string_pretty(file).map_err(|e| EmbedError::Config(e.to_string()))?;
    std::fs::write(&path, s).map_err(|e| EmbedError::Config(e.to_string()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut p = std::fs::metadata(&path).unwrap().permissions();
        p.set_mode(0o600);
        let _ = std::fs::set_permissions(&path, p);
    }
    Ok(())
}

pub fn remove_embed_file() -> Result<(), EmbedError> {
    let path = embed_config_path();
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| EmbedError::Config(e.to_string()))?;
    }
    Ok(())
}

/// Load config from disk; returns `None` if missing or invalid (caller may ignore).
pub fn try_load_embed_file() -> Option<EmbedFile> {
    load_embed_file()
}
