//! hellodb-brain — passive memory pipeline for hellodb.
//!
//! Run this binary (from a cron / launchd / stop-hook / manual) to:
//!   1. Tail the `episodes` namespace since the last cursor
//!   2. Evaluate gates (cool-down + minimum episode count)
//!   3. Digest new episodes into consolidated facts via a pluggable backend
//!   4. Write facts to a `{facts}/digest-{timestamp}` draft branch
//!   5. Update state; user merges approved digests via `/hellodb-review`
//!
//! The reversed-dependency pattern: primary agent never triggers memory
//! operations, never knows the brain exists. Brain observes via tail.
//!
//! CLI:
//!     hellodb-brain                 # run one pass using $HELLODB_HOME/brain.toml
//!     hellodb-brain --config path   # explicit config
//!     hellodb-brain --dry-run       # digest + print, don't write to DB
//!     hellodb-brain --force         # skip gate evaluation (still uses lock)
//!     hellodb-brain --status        # print state + config, no pass
//!     hellodb-brain --init-config   # write default brain.toml to data_dir

mod config;
mod digest;
mod error;
mod gates;
mod lock;
mod state;

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use hellodb_core::{Branch, FieldType, Namespace, Record, Schema, SchemaField};
use hellodb_crypto::{
    content_hash, content_hash_bytes, KeyPair, MasterKey, NamespaceKey, SigningKey,
};
use hellodb_storage::{SqliteEngine, StorageEngine};
use hellodb_vector::VectorIndex;

use crate::config::Config;
use crate::digest::Fact;
use crate::error::BrainError;
use crate::gates::GateDecision;
use crate::state::State;

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

fn main() {
    std::process::exit(match run() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("hellodb-brain error: {e}");
            1
        }
    });
}

struct CliFlags {
    config_path: Option<PathBuf>,
    dry_run: bool,
    force: bool,
    status: bool,
    init_config: bool,
}

fn parse_flags() -> CliFlags {
    let mut flags = CliFlags {
        config_path: None,
        dry_run: false,
        force: false,
        status: false,
        init_config: false,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" => flags.config_path = args.next().map(PathBuf::from),
            "--dry-run" => flags.dry_run = true,
            "--force" => flags.force = true,
            "--status" => flags.status = true,
            "--init-config" => flags.init_config = true,
            "-h" | "--help" => {
                print_help();
                std::process::exit(0);
            }
            other => {
                eprintln!("unknown flag: {other}");
                print_help();
                std::process::exit(2);
            }
        }
    }
    flags
}

fn print_help() {
    eprintln!("hellodb-brain — passive memory pipeline");
    eprintln!();
    eprintln!("flags:");
    eprintln!("  --config <path>   config file (default: $HELLODB_HOME/brain.toml)");
    eprintln!("  --dry-run         digest and print, don't write");
    eprintln!("  --force           skip gate evaluation (still takes the lock)");
    eprintln!("  --status          print state + config, then exit");
    eprintln!("  --init-config     write default brain.toml to the data dir");
}

fn data_dir() -> PathBuf {
    if let Ok(override_) = std::env::var("HELLODB_HOME") {
        return PathBuf::from(override_);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".hellodb")
}

fn run() -> Result<i32, BrainError> {
    let flags = parse_flags();
    let data = data_dir();
    let config_path = flags
        .config_path
        .clone()
        .unwrap_or_else(|| data.join("brain.toml"));

    // --init-config: write defaults, exit.
    if flags.init_config {
        if config_path.exists() {
            eprintln!(
                "refusing to overwrite existing config at {}",
                config_path.display()
            );
            return Ok(2);
        }
        std::fs::create_dir_all(&data)?;
        let cfg = Config::with_defaults(&data);
        let s = toml::to_string_pretty(&cfg)
            .map_err(|e| BrainError::Config(format!("serializing defaults: {e}")))?;
        std::fs::write(&config_path, s)?;
        println!("wrote default config to {}", config_path.display());
        return Ok(0);
    }

    // Load config: explicit file if present, else generate defaults.
    let config = if config_path.exists() {
        Config::load(&config_path)?
    } else {
        eprintln!(
            "no config at {}, using built-in defaults. (run --init-config to write one.)",
            config_path.display()
        );
        Config::with_defaults(&data)
    };

    let state = State::load(&config.data.state_path)?;

    // --status: print and exit.
    if flags.status {
        print_status(&config, &state);
        return Ok(0);
    }

    // Acquire lock for the full pass.
    let _lock = lock::LockGuard::acquire(&config.data.lock_path)?;

    // Run one pass.
    let report = run_one_pass(&config, state, &flags)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(0)
}

fn print_status(config: &Config, state: &State) {
    println!(
        "{}",
        serde_json::json!({
            "config": {
                "episodes_namespace": config.namespaces.episodes,
                "facts_namespace": config.namespaces.facts,
                "gates": {
                    "min_time_ms": config.gates.min_time_since_last_run_ms,
                    "min_episodes": config.gates.min_episodes_since_last_run,
                },
                "backend": config.digest.backend,
                "db_path": config.data.db_path.display().to_string(),
            },
            "state": state,
            "now_ms": now_ms(),
        })
    );
}

/// The outcome of a single pass, emitted as JSON to stdout.
#[derive(Debug, serde::Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum PassReport {
    Skipped {
        reason: String,
        episode_count: usize,
        cursor: u64,
    },
    DryRun {
        facts: Vec<Fact>,
        episode_count: usize,
        would_write_to_branch: String,
    },
    Digested {
        facts_written: usize,
        facts_indexed: usize,
        branch_id: String,
        episode_count: usize,
        new_cursor: u64,
    },
    NoEpisodes {
        cursor: u64,
    },
}

fn run_one_pass(
    config: &Config,
    mut state: State,
    flags: &CliFlags,
) -> Result<PassReport, BrainError> {
    let keypair = load_identity(&config.data.identity_path)?;
    let sqlcipher_key = derive_sqlcipher_key(&keypair.signing);

    let mut storage = SqliteEngine::open(
        config
            .data
            .db_path
            .to_str()
            .ok_or_else(|| BrainError::Config("db_path is not valid utf-8".into()))?,
        &sqlcipher_key,
    )?;

    // Update last_attempt regardless of outcome.
    state.last_attempt_ms = now_ms();

    // Tail episodes since last cursor, capped at max_episodes_per_pass.
    let episodes = storage.tail_records(
        &config.namespaces.episodes,
        state.last_cursor,
        config.limits.max_episodes_per_pass,
        None,
    )?;

    if episodes.is_empty() {
        state.last_skip_reason = Some("no new episodes".into());
        state.save(&config.data.state_path)?;
        return Ok(PassReport::NoEpisodes {
            cursor: state.last_cursor,
        });
    }

    let new_cursor = episodes
        .iter()
        .map(|e| e.seq)
        .max()
        .unwrap_or(state.last_cursor);

    // Gates (unless --force).
    if !flags.force {
        let decision = gates::evaluate(&state, episodes.len(), &config.gates, now_ms());
        if let GateDecision::Skip(reason) = decision {
            state.last_skip_reason = Some(reason.clone());
            state.save(&config.data.state_path)?;
            return Ok(PassReport::Skipped {
                reason,
                episode_count: episodes.len(),
                cursor: state.last_cursor,
            });
        }
    }

    // Digest.
    let backend = digest::select_backend(&config.digest.backend)?;
    let facts = backend.digest(&episodes, config)?;

    // Dry-run: print and stop.
    if flags.dry_run {
        return Ok(PassReport::DryRun {
            facts,
            episode_count: episodes.len(),
            would_write_to_branch: format!("{}/digest-{}", config.namespaces.facts, now_ms()),
        });
    }

    // Ensure facts namespace + fact schema exist (idempotent).
    ensure_facts_namespace(&mut storage, &keypair, config)?;

    // Create the digest branch off facts/main.
    let now = now_ms();
    let branch_id = format!("{}/digest-{}", config.namespaces.facts, now);
    let parent_id = format!("{}/main", config.namespaces.facts);
    let branch = Branch::new(
        branch_id.clone(),
        config.namespaces.facts.clone(),
        parent_id,
        format!("digest-{now}"),
    );
    storage.create_branch(branch)?;

    // Optional: build an embedder from env. Failure here is non-fatal —
    // facts still get written, they just don't get indexed for semantic
    // recall this pass. (That keeps brain robust when the user hasn't
    // configured an embed backend yet.)
    let embedder = match hellodb_embed::build_from_env() {
        Ok(e) => Some(e),
        Err(hellodb_embed::EmbedError::Config(_)) => None,
        Err(e) => {
            eprintln!("brain: embedder init failed, skipping indexing: {e}");
            None
        }
    };

    // Write each fact as a signed record on the draft branch. If an
    // embedder is configured, also embed the `statement` and upsert
    // into the namespace's vector index so hellodb_embed_and_search
    // picks it up at recall time.
    let mut facts_written = 0;
    let mut facts_indexed = 0;
    let vector_dir = config
        .data
        .db_path
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .join("vectors");

    for fact in &facts {
        let data = serde_json::to_value(fact)?;
        let record = Record::new(
            &keypair.signing,
            config.digest.fact_schema_id.clone(),
            config.namespaces.facts.clone(),
            data,
            None,
        )?;
        let record_id = record.record_id.clone();
        storage.put_record(record, &branch_id)?;
        facts_written += 1;

        if let Some(ref embedder) = embedder {
            match embedder.embed_one(&fact.statement) {
                Ok(v) => {
                    let vec_key =
                        derive_namespace_vector_key(&keypair.signing, &config.namespaces.facts);
                    match VectorIndex::open(&vector_dir, &config.namespaces.facts, &vec_key) {
                        Ok(mut idx) => {
                            if let Err(e) = idx.upsert(record_id, v) {
                                eprintln!("brain: vector upsert failed: {e}");
                            } else {
                                facts_indexed += 1;
                            }
                        }
                        Err(e) => eprintln!("brain: vector index open failed: {e}"),
                    }
                }
                Err(e) => eprintln!("brain: embed failed for fact '{}': {e}", fact.topic),
            }
        }
    }

    // Update state — only successful digests advance last_run_ms.
    state.last_cursor = new_cursor;
    state.last_run_ms = now;
    state.run_count += 1;
    state.last_skip_reason = None;
    state.save(&config.data.state_path)?;

    Ok(PassReport::Digested {
        facts_written,
        facts_indexed,
        branch_id,
        episode_count: episodes.len(),
        new_cursor,
    })
}

fn ensure_facts_namespace(
    storage: &mut SqliteEngine,
    keypair: &KeyPair,
    config: &Config,
) -> Result<(), BrainError> {
    if storage.get_namespace(&config.namespaces.facts)?.is_none() {
        let mut ns = Namespace::new(
            config.namespaces.facts.clone(),
            config.namespaces.facts.clone(),
            keypair.verifying.clone(),
            true,
        );
        ns.description = Some("consolidated facts produced by hellodb-brain".into());
        storage.create_namespace(ns)?;
    }

    if storage.get_schema(&config.digest.fact_schema_id)?.is_none() {
        let schema = Schema {
            id: config.digest.fact_schema_id.clone(),
            version: "1.0.0".into(),
            namespace: config.namespaces.facts.clone(),
            name: "Brain fact".into(),
            fields: vec![
                SchemaField {
                    name: "statement".into(),
                    field_type: FieldType::String,
                    required: true,
                    description: Some("Canonical one-sentence statement".into()),
                },
                SchemaField {
                    name: "topic".into(),
                    field_type: FieldType::String,
                    required: true,
                    description: None,
                },
                SchemaField {
                    name: "confidence".into(),
                    field_type: FieldType::Float,
                    required: true,
                    description: Some("Backend-reported confidence in [0, 1]".into()),
                },
                SchemaField {
                    name: "derived_from".into(),
                    field_type: FieldType::Array(Box::new(FieldType::String)),
                    required: false,
                    description: Some("Episode record ids this fact is derived from".into()),
                },
                SchemaField {
                    name: "rationale".into(),
                    field_type: FieldType::Optional(Box::new(FieldType::String)),
                    required: false,
                    description: None,
                },
            ],
            registered_at_ms: now_ms(),
        };
        storage.register_schema(schema)?;
    }
    Ok(())
}

fn load_identity(path: &Path) -> Result<KeyPair, BrainError> {
    let bytes = std::fs::read(path)
        .map_err(|e| BrainError::Identity(format!("reading {}: {e}", path.display())))?;
    if bytes.len() != 32 {
        return Err(BrainError::Identity(format!(
            "identity.key must be 32 bytes, got {}",
            bytes.len()
        )));
    }
    let mut seed = [0u8; 32];
    seed.copy_from_slice(&bytes);
    let signing = SigningKey::from_bytes(&seed);
    let verifying = signing.verifying_key();
    Ok(KeyPair { signing, verifying })
}

/// MUST match `hellodb-mcp`'s `derive_sqlcipher_key` (in identity.rs) so the
/// brain can open the same encrypted database. If you ever change one, change
/// both — mismatched keys cause SQLCipher to return "file is not a database".
fn derive_sqlcipher_key(signing: &SigningKey) -> String {
    let seed = signing.to_bytes();
    let mut input = Vec::with_capacity(64);
    input.extend_from_slice(b"hellodb-mcp-sqlcipher-v1:");
    input.extend_from_slice(&seed);
    content_hash(&input)
}

/// MUST match `hellodb-mcp`'s `derive_namespace_vector_key` so brain and MCP
/// open the same encrypted per-namespace vector index.
fn derive_namespace_vector_key(signing: &SigningKey, namespace: &str) -> NamespaceKey {
    let seed = signing.to_bytes();
    let mut input =
        Vec::with_capacity(64 + namespace.len() + "hellodb-mcp-vector-key-v1:".len() + 1);
    input.extend_from_slice(b"hellodb-mcp-vector-key-v1:");
    input.extend_from_slice(namespace.as_bytes());
    input.extend_from_slice(b":");
    input.extend_from_slice(&seed);
    let derived = content_hash_bytes(&input);
    MasterKey::from_bytes(derived).derive_namespace_key(namespace)
}
