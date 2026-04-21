//! hellodb — unified CLI entry point.
//!
//! One command, clean subcommand surface:
//!
//!   hellodb init              — first-time setup (identity, brain.toml)
//!   hellodb status            — identity + namespaces + brain state
//!   hellodb recall [opts]     — rank curated facts by decayed score, emit markdown/json
//!   hellodb ingest [opts]     — import Claude Code's auto-memory markdown files into hellodb
//!   hellodb mcp               — run the MCP server (stdio, for Claude Code)
//!   hellodb brain [args...]   — run the passive-memory digest pass
//!   hellodb doctor            — diagnose config/permission/DB-open issues
//!
//! The mcp/brain subcommands exec the companion binaries (`hellodb-mcp` and
//! `hellodb-brain`) that ship beside this one. That keeps responsibilities
//! crisp: this CLI is the friendly front door; the runtime binaries haven't
//! changed. When shipped as a plugin bundle, all three sit in the plugin's
//! `bin/` directory together.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

use std::time::{SystemTime, UNIX_EPOCH};

use hellodb_core::Record;
use hellodb_crypto::{content_hash, KeyPair, SigningKey};
use hellodb_storage::{decayed_score, SqliteEngine, StorageEngine};

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(String::as_str) {
        Some("init") => cmd_init(&args[1..]),
        Some("status") => cmd_status(&args[1..]),
        Some("recall") => cmd_recall(&args[1..]),
        Some("ingest") => cmd_ingest(&args[1..]),
        Some("mcp") => cmd_exec_sibling("hellodb-mcp", &args[1..]),
        Some("brain") => cmd_exec_sibling("hellodb-brain", &args[1..]),
        Some("integrate") => cmd_integrate(&args[1..]),
        Some("doctor") => cmd_doctor(),
        Some("-h") | Some("--help") | Some("help") | None => {
            print_help();
            Ok(0)
        }
        Some(other) => {
            eprintln!("unknown subcommand: {other}");
            print_help();
            Ok(2)
        }
    };
    std::process::exit(match code {
        Ok(n) => n,
        Err(e) => {
            eprintln!("hellodb: {e}");
            1
        }
    });
}

fn print_help() {
    let exe = env!("CARGO_PKG_VERSION");
    println!("hellodb {exe} — sovereign, encrypted, branchable memory for agents");
    println!();
    println!("usage: hellodb <subcommand> [args...]");
    println!();
    println!("subcommands:");
    println!("  init       first-time setup: data dir, identity key, brain.toml");
    println!("  status     show identity, namespaces, record counts, brain state");
    println!("  recall     top facts ranked by decayed score (markdown or json)");
    println!("             flags: --top N (default 8), --namespace NS (default claude.facts),");
    println!("                    --format md|json (default md), --half-life-days D (default 7),");
    println!("                    --verbose (show errors on stderr; default is silent)");
    println!("  ingest     import Claude Code auto-memory markdown into hellodb");
    println!("             flags: --from-claudemd (scan ~/.claude/projects/*/memory/*.md),");
    println!("                    --source PATH (explicit memory dir), --dry-run");
    println!("  mcp        run the MCP server (stdio transport; for Claude Code)");
    println!("  brain      run one passive-memory digest pass (see --help for flags)");
    println!("  integrate  wire hellodb-mcp into a host (currently: codex)");
    println!("  doctor     diagnose common setup issues");
    println!();
    println!("environment:");
    println!("  HELLODB_HOME   data dir (default: ~/.hellodb)");
}

// --- init -----------------------------------------------------------------

fn cmd_init(_args: &[String]) -> Result<i32, String> {
    let data = data_dir();
    std::fs::create_dir_all(&data).map_err(|e| format!("creating {}: {e}", data.display()))?;

    let identity_path = data.join("identity.key");
    let created_identity = !identity_path.exists();
    if created_identity {
        let kp = KeyPair::generate();
        std::fs::write(&identity_path, kp.signing.to_bytes())
            .map_err(|e| format!("writing identity: {e}"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut p = std::fs::metadata(&identity_path).unwrap().permissions();
            p.set_mode(0o600);
            std::fs::set_permissions(&identity_path, p).unwrap();
        }
    }

    // Open DB to force creation + schema bootstrap.
    let keypair = load_identity(&identity_path)?;
    let db_path = data.join("local.db");
    let db_key = derive_sqlcipher_key(&keypair.signing);
    let _ = SqliteEngine::open(path_as_str(&db_path)?, &db_key)
        .map_err(|e| format!("opening db: {e}"))?;

    // Write default brain.toml if missing.
    let brain_toml = data.join("brain.toml");
    let wrote_brain_toml = !brain_toml.exists();
    if wrote_brain_toml {
        let contents = default_brain_toml(&data);
        std::fs::write(&brain_toml, contents).map_err(|e| format!("writing brain.toml: {e}"))?;
    }

    println!(
        "{}",
        serde_json::json!({
            "data_dir": data.display().to_string(),
            "identity_fingerprint": keypair.verifying.fingerprint(),
            "identity_created": created_identity,
            "db_opened": true,
            "brain_config_written": wrote_brain_toml,
            "next_steps": [
                "hellodb status",
                "claude mcp add hellodb ${path_to_hellodb_binary_or_hellodb_mcp}",
                "hellodb integrate codex   # if you use OpenAI Codex instead",
                "hellodb brain --status",
            ],
        })
    );
    Ok(0)
}

fn default_brain_toml(data: &Path) -> String {
    format!(
        r#"[data]
db_path = "{db}"
identity_path = "{id}"
state_path = "{state}"
lock_path = "{lock}"

[namespaces]
episodes = "claude.episodes"
facts = "claude.facts"

[gates]
# 6 hours cool-down between runs
min_time_since_last_run_ms = 21600000
# need at least 5 new episodes to justify a digest call
min_episodes_since_last_run = 5

[limits]
max_episodes_per_pass = 200
# Max UTF-8 characters of raw episode `data` serialised into a single digest
# prompt row. Oversize episodes are head-truncated so the LLM driving the
# digest still sees their intent without absorbing their full tail.
max_episode_chars = 2000
# Hard ceiling on total prompt size. Episodes beyond this are deferred to
# the next pass (the brain cursor only advances on success, so nothing is
# lost — it's just back-pressure).
max_prompt_chars = 200000

[digest]
# Supported backends:
# - mock       (deterministic, no remote model)
# - openrouter (set HELLODB_BRAIN_OPENROUTER_API_KEY)
backend = "mock"
fact_schema_id = "brain.fact"
# Confidence at or above which facts auto-merge to main. Set to 1.1 to hold
# every fact for manual review (via /hellodb:review). `supersedes` facts
# (contradictions) always wait for review regardless of this value.
auto_merge_threshold = 0.75
"#,
        db = data.join("local.db").display(),
        id = data.join("identity.key").display(),
        state = data.join("brain.state.json").display(),
        lock = data.join("brain.lock").display(),
    )
}

// --- status ---------------------------------------------------------------

fn cmd_status(_args: &[String]) -> Result<i32, String> {
    let data = data_dir();
    let identity_path = data.join("identity.key");
    if !identity_path.exists() {
        println!(
            "{}",
            serde_json::json!({
                "initialized": false,
                "data_dir": data.display().to_string(),
                "hint": "run `hellodb init` first",
            })
        );
        return Ok(0);
    }

    let keypair = load_identity(&identity_path)?;
    let db_path = data.join("local.db");
    let db_key = derive_sqlcipher_key(&keypair.signing);
    let storage = SqliteEngine::open(path_as_str(&db_path)?, &db_key)
        .map_err(|e| format!("opening db: {e}"))?;

    let namespaces = storage
        .list_namespaces()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|ns| {
            let branches = storage.list_branches(&ns.id).unwrap_or_default();
            let main = format!("{}/main", ns.id);
            let main_count = storage
                .list_records_by_namespace(&ns.id, &main, usize::MAX, 0)
                .map(|r| r.len())
                .unwrap_or(0);
            serde_json::json!({
                "id": ns.id,
                "encrypted": ns.encrypted,
                "is_owner": ns.owner == keypair.verifying,
                "schemas": ns.schemas,
                "branch_count": branches.len(),
                "main_record_count": main_count,
                "active_digest_branches": branches
                    .iter()
                    .filter(|b| b.label.starts_with("digest-") && b.state == hellodb_core::BranchState::Active)
                    .count(),
            })
        })
        .collect::<Vec<_>>();

    let brain_state = {
        let path = data.join("brain.state.json");
        if path.exists() {
            std::fs::read_to_string(&path)
                .ok()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        } else {
            None
        }
    };

    println!(
        "{}",
        serde_json::json!({
            "initialized": true,
            "data_dir": data.display().to_string(),
            "identity": {
                "fingerprint": keypair.verifying.fingerprint(),
                "pubkey_b64": keypair.verifying.to_base64(),
            },
            "db_path": db_path.display().to_string(),
            "namespaces": namespaces,
            "brain_state": brain_state,
            "brain_config_present": data.join("brain.toml").exists(),
        })
    );

    // Silence "unused" by referencing Record at least once — documents intent
    // that future status extensions will surface record-level info.
    let _ = std::marker::PhantomData::<Record>;
    Ok(0)
}

// --- recall ---------------------------------------------------------------

/// Rank records in a facts-style namespace by their decay-adjusted score,
/// emit the top-N as markdown (for hook injection / human reading) or JSON
/// (for programmatic consumption by another tool).
///
/// Non-fatal on every error the session start hook might hit: if the DB is
/// absent, empty, or unreadable, we print nothing and exit 0. Silence is the
/// intentional response — a broken recall should not break the user's
/// Claude Code session start.
fn print_recall_help() {
    println!("hellodb recall — top facts ranked by decayed reinforcement score");
    println!();
    println!("usage: hellodb recall [flags]");
    println!();
    println!("flags:");
    println!("  --top N             number of facts to return (default 8)");
    println!("  --namespace NS      namespace to rank over (default claude.facts)");
    println!("  --format md|json    output shape (default md)");
    println!("  --half-life-days D  decay half-life in days (default 7)");
    println!("  --verbose, -v       show errors on stderr (default: silent — safe for hooks)");
    println!("  --quiet             explicit quiet mode (default)");
    println!("  -h, --help          this help");
}

fn cmd_recall(args: &[String]) -> Result<i32, String> {
    let mut top: usize = 8;
    let mut namespace = "claude.facts".to_string();
    let mut format = "md".to_string();
    let mut half_life_days: f64 = 7.0;
    // Default to quiet because this command is designed to be wired into
    // hooks and plugin-agent pipelines — stderr noise during session bootstrap
    // would be visible to the user. Opt into chatty mode with --verbose when
    // debugging from the terminal.
    let mut quiet = true;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                print_recall_help();
                return Ok(0);
            }
            "--top" => {
                top = args.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(8);
                i += 2;
            }
            "--namespace" => {
                namespace = args.get(i + 1).cloned().unwrap_or(namespace);
                i += 2;
            }
            "--format" => {
                format = args.get(i + 1).cloned().unwrap_or(format);
                i += 2;
            }
            "--half-life-days" => {
                half_life_days = args.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(7.0);
                i += 2;
            }
            "--quiet" => {
                quiet = true;
                i += 1;
            }
            "--verbose" | "-v" => {
                quiet = false;
                i += 1;
            }
            other => {
                eprintln!("recall: unknown flag {other}");
                return Ok(2);
            }
        }
    }

    let data = data_dir();
    let identity_path = data.join("identity.key");
    if !identity_path.exists() {
        if !quiet {
            eprintln!("recall: not initialized (no identity.key). run `hellodb init` first.");
        }
        return Ok(0);
    }

    let keypair = match load_identity(&identity_path) {
        Ok(kp) => kp,
        Err(e) => {
            if !quiet {
                eprintln!("recall: {e}");
            }
            return Ok(0);
        }
    };

    let db_path = data.join("local.db");
    let db_key = derive_sqlcipher_key(&keypair.signing);
    // Path error is ALWAYS a misconfig (HELLODB_HOME set to a non-UTF-8
    // path) — never a "no results" condition. Surface it even under
    // `--quiet` so users don't silently get empty output while thinking
    // recall just found nothing. `--quiet` is meant to suppress
    // informational noise, not mask setup bugs.
    let db_path_str = match path_as_str(&db_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("recall: {e}");
            return Ok(2);
        }
    };
    let storage = match SqliteEngine::open(db_path_str, &db_key) {
        Ok(s) => s,
        Err(e) => {
            if !quiet {
                eprintln!("recall: couldn't open db: {e}");
            }
            return Ok(0);
        }
    };

    // Skip silently if the namespace doesn't exist yet — common on fresh installs.
    if storage
        .get_namespace(&namespace)
        .map_err(|e| e.to_string())?
        .is_none()
    {
        return Ok(0);
    }

    let branch = format!("{namespace}/main");
    let records = storage
        .list_records_by_namespace(&namespace, &branch, 10_000, 0)
        .map_err(|e| e.to_string())?;

    // Pair each record with its decay-adjusted score. Unreinforced records
    // (no metadata row) default to 0.0 so actively-used facts rise above
    // the unread baseline.
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let half_life_ms = (half_life_days * 86_400_000.0) as u64;

    let mut scored: Vec<(f32, Record)> = records
        .into_iter()
        .filter(|r| {
            // Skip archived records so recall doesn't resurface aged-out memory.
            storage
                .get_record_metadata(&r.record_id)
                .ok()
                .flatten()
                .and_then(|m| m.archived_at_ms)
                .is_none()
        })
        .map(|r| {
            let score = storage
                .get_record_metadata(&r.record_id)
                .ok()
                .flatten()
                .map(|m| decayed_score(&m, now, half_life_ms))
                .unwrap_or(0.0);
            (score, r)
        })
        .collect();

    // Descending by decayed score; ties broken by newer created_at_ms first.
    scored.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.1.created_at_ms.cmp(&a.1.created_at_ms))
    });
    scored.truncate(top);

    if scored.is_empty() {
        // Nothing to recall — print empty string in md mode (hook just injects nothing)
        // or empty array in json mode. Either way, exit 0 silently.
        if format == "json" {
            println!("[]");
        }
        return Ok(0);
    }

    match format.as_str() {
        "json" => {
            let items: Vec<_> = scored
                .iter()
                .map(|(s, r)| {
                    serde_json::json!({
                        "record_id": r.record_id,
                        "topic": r.data.get("topic").and_then(|v| v.as_str()).unwrap_or(""),
                        "statement": r.data.get("statement").and_then(|v| v.as_str()).unwrap_or(""),
                        "confidence": r.data.get("confidence").and_then(|v| v.as_f64()).unwrap_or(0.0),
                        "decayed_score": s,
                    })
                })
                .collect();
            println!(
                "{}",
                serde_json::to_string(&items).unwrap_or_else(|_| "[]".into())
            );
        }
        _ => {
            // Markdown bullet list. Group by topic so context is readable at a glance.
            use std::collections::BTreeMap;
            let mut by_topic: BTreeMap<String, Vec<(&f32, &Record)>> = BTreeMap::new();
            for (s, r) in &scored {
                let topic = r
                    .data
                    .get("topic")
                    .and_then(|v| v.as_str())
                    .unwrap_or("other")
                    .to_string();
                by_topic.entry(topic).or_default().push((s, r));
            }
            for (topic, facts) in &by_topic {
                println!("**{topic}**");
                for (score, r) in facts {
                    // Try the common text-bearing fields across schemas:
                    // brain.fact uses `statement`; episodes use `text`;
                    // legacy feedback uses `rule`; notes use `content`.
                    let statement = r
                        .data
                        .get("statement")
                        .and_then(|v| v.as_str())
                        .or_else(|| r.data.get("text").and_then(|v| v.as_str()))
                        .or_else(|| r.data.get("rule").and_then(|v| v.as_str()))
                        .or_else(|| r.data.get("content").and_then(|v| v.as_str()))
                        .unwrap_or("(no text)");
                    println!("- {statement}  _(score {:.2})_", score);
                }
                println!();
            }
        }
    }

    Ok(0)
}

// --- ingest ---------------------------------------------------------------

/// Import Claude Code's auto-memory markdown files into hellodb.
///
/// Claude Code writes per-project memory files to
/// `~/.claude/projects/<sanitized-path>/memory/*.md` with YAML frontmatter
/// declaring a `type` (user / feedback / project / reference) and an
/// optional `description`. See claude-code/src/memdir/memoryScan.ts for
/// the format.
///
/// This subcommand walks those directories, parses the frontmatter, and
/// creates signed records in hellodb under a namespace-per-project. The
/// record's `data` carries:
///   - type:       one of the four memory types
///   - description: the frontmatter description (if any)
///   - body:       the file content below the frontmatter
///   - source_path: absolute path of the .md file
///   - project:    the sanitized-path segment from ~/.claude/projects/
///   - mtime_ms:   file mtime (so recall can rank by freshness)
///
/// Schema id used: `claude.memory.<type>` (auto-registered on first use).
///
/// Dedup: content-addressed record_ids mean re-running on unchanged files
/// is a no-op. If a file is edited, the new content gets a new record_id
/// and supersedes via `previous_version` — nothing is lost.
///
/// `--dry-run` reports what would be ingested without writing.
fn cmd_ingest(args: &[String]) -> Result<i32, String> {
    let mut from_claudemd = false;
    let mut source_path: Option<PathBuf> = None;
    let mut dry_run = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                print_ingest_help();
                return Ok(0);
            }
            "--from-claudemd" => {
                from_claudemd = true;
                i += 1;
            }
            "--source" => {
                source_path = args.get(i + 1).map(PathBuf::from);
                i += 2;
            }
            "--dry-run" => {
                dry_run = true;
                i += 1;
            }
            other => {
                eprintln!("ingest: unknown flag {other}");
                return Ok(2);
            }
        }
    }

    if !from_claudemd && source_path.is_none() {
        eprintln!("ingest: specify --from-claudemd or --source <path>");
        print_ingest_help();
        return Ok(2);
    }

    // Enumerate candidate memory directories.
    let dirs: Vec<PathBuf> = if let Some(src) = source_path {
        if !src.exists() {
            return Err(format!("source path does not exist: {}", src.display()));
        }
        vec![src]
    } else {
        // Scan ~/.claude/projects/*/memory/
        let projects_root = match std::env::var("HOME") {
            Ok(home) => PathBuf::from(home).join(".claude").join("projects"),
            Err(_) => return Err("HOME unset; can't locate ~/.claude/projects".into()),
        };
        if !projects_root.exists() {
            println!(
                "{}",
                serde_json::json!({
                    "status": "nothing_to_ingest",
                    "reason": "no ~/.claude/projects directory",
                    "hint": "run Claude Code at least once to populate auto-memory"
                })
            );
            return Ok(0);
        }
        let mut out = Vec::new();
        match std::fs::read_dir(&projects_root) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    let mem_dir = entry.path().join("memory");
                    if mem_dir.is_dir() {
                        out.push(mem_dir);
                    }
                }
            }
            Err(e) => return Err(format!("reading {}: {e}", projects_root.display())),
        }
        out
    };

    if dirs.is_empty() {
        println!(
            "{}",
            serde_json::json!({
                "status": "nothing_to_ingest",
                "reason": "no memory directories found under ~/.claude/projects/"
            })
        );
        return Ok(0);
    }

    // Ensure hellodb is initialized.
    let data_dir = data_dir();
    let identity_path = data_dir.join("identity.key");
    if !identity_path.exists() {
        return Err("hellodb not initialized (no identity.key). run `hellodb init` first.".into());
    }
    let keypair = load_identity(&identity_path)?;
    let db_path = data_dir.join("local.db");
    let db_key = derive_sqlcipher_key(&keypair.signing);

    // Walk every memory dir.
    let mut files_scanned = 0usize;
    let mut records_ingested = 0usize;
    let mut namespaces_touched: std::collections::BTreeSet<String> =
        std::collections::BTreeSet::new();
    let mut errors: Vec<String> = Vec::new();
    let mut per_project: Vec<serde_json::Value> = Vec::new();

    // Open storage once (unless dry-run).
    let mut storage = if dry_run {
        None
    } else {
        Some(
            SqliteEngine::open(path_as_str(&db_path)?, &db_key)
                .map_err(|e| format!("opening db: {e}"))?,
        )
    };

    for mem_dir in &dirs {
        let project_slug = mem_dir
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();
        let namespace = format!("claude.memory.{}", sanitize_ns_segment(&project_slug));

        // Ensure namespace + a permissive schema exist for each memory type.
        if let Some(ref mut s) = storage {
            if s.get_namespace(&namespace).ok().flatten().is_none() {
                let mut ns = hellodb_core::Namespace::new(
                    namespace.clone(),
                    project_slug.clone(),
                    keypair.verifying.clone(),
                    true,
                );
                ns.description = Some(format!(
                    "Imported from Claude Code auto-memory for project {project_slug}"
                ));
                s.create_namespace(ns).map_err(|e| e.to_string())?;
            }
            // One schema with flexible fields covers all four types; validation is
            // by the `type` field's value, not by distinct schemas.
            let schema_id = format!("{namespace}.claudemd");
            if s.get_schema(&schema_id).ok().flatten().is_none() {
                let schema = hellodb_core::Schema {
                    id: schema_id.clone(),
                    version: "1.0.0".into(),
                    namespace: namespace.clone(),
                    name: "Claude Code auto-memory file".into(),
                    fields: vec![
                        hellodb_core::SchemaField {
                            name: "type".into(),
                            field_type: hellodb_core::FieldType::String,
                            required: true,
                            description: Some("user | feedback | project | reference".into()),
                        },
                        hellodb_core::SchemaField {
                            name: "description".into(),
                            field_type: hellodb_core::FieldType::Optional(Box::new(
                                hellodb_core::FieldType::String,
                            )),
                            required: false,
                            description: None,
                        },
                        hellodb_core::SchemaField {
                            name: "body".into(),
                            field_type: hellodb_core::FieldType::String,
                            required: true,
                            description: None,
                        },
                        hellodb_core::SchemaField {
                            name: "source_path".into(),
                            field_type: hellodb_core::FieldType::String,
                            required: true,
                            description: None,
                        },
                        hellodb_core::SchemaField {
                            name: "project".into(),
                            field_type: hellodb_core::FieldType::String,
                            required: true,
                            description: None,
                        },
                        hellodb_core::SchemaField {
                            name: "mtime_ms".into(),
                            field_type: hellodb_core::FieldType::Timestamp,
                            required: true,
                            description: None,
                        },
                    ],
                    registered_at_ms: now_ms(),
                };
                s.register_schema(schema).map_err(|e| e.to_string())?;
            }
        }

        // Walk .md files in this dir.
        let files = match std::fs::read_dir(mem_dir) {
            Ok(it) => it,
            Err(e) => {
                errors.push(format!("reading {}: {e}", mem_dir.display()));
                continue;
            }
        };
        let mut this_project_ingested = 0usize;
        for f in files.flatten() {
            let p = f.path();
            if p.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            // Claude Code reserves MEMORY.md as an index file — skip it.
            if p.file_name().and_then(|n| n.to_str()) == Some("MEMORY.md") {
                continue;
            }
            files_scanned += 1;
            let body_text = match std::fs::read_to_string(&p) {
                Ok(s) => s,
                Err(e) => {
                    errors.push(format!("reading {}: {e}", p.display()));
                    continue;
                }
            };
            let mtime_ms = std::fs::metadata(&p)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);

            let (frontmatter, body) = parse_frontmatter(&body_text);
            let mem_type = frontmatter
                .get("type")
                .cloned()
                .unwrap_or_else(|| "reference".into());
            let description = frontmatter.get("description").cloned();

            if dry_run {
                continue;
            }

            let data = serde_json::json!({
                "type": mem_type,
                "description": description,
                "body": body,
                "source_path": p.display().to_string(),
                "project": project_slug,
                "mtime_ms": mtime_ms,
            });

            let schema_id = format!("{namespace}.claudemd");
            let record = hellodb_core::Record::new_with_timestamp(
                &keypair.signing,
                schema_id,
                namespace.clone(),
                data,
                None,
                mtime_ms,
            )
            .map_err(|e| e.to_string())?;

            let main_branch = format!("{namespace}/main");
            if let Some(ref mut s) = storage {
                s.put_record(record, &main_branch)
                    .map_err(|e| e.to_string())?;
                records_ingested += 1;
                this_project_ingested += 1;
            }
        }
        namespaces_touched.insert(namespace.clone());
        per_project.push(serde_json::json!({
            "namespace": namespace,
            "project": project_slug,
            "source_dir": mem_dir.display().to_string(),
            "records_ingested": this_project_ingested,
        }));
    }

    println!(
        "{}",
        serde_json::json!({
            "status": if dry_run { "dry_run" } else { "ingested" },
            "files_scanned": files_scanned,
            "records_ingested": records_ingested,
            "namespaces": namespaces_touched.into_iter().collect::<Vec<_>>(),
            "projects": per_project,
            "errors": errors,
        })
    );
    Ok(0)
}

fn print_ingest_help() {
    println!("hellodb ingest — import Claude Code auto-memory into hellodb");
    println!();
    println!("usage: hellodb ingest [--from-claudemd | --source PATH] [--dry-run]");
    println!();
    println!("flags:");
    println!("  --from-claudemd   scan ~/.claude/projects/*/memory/*.md (requires HOME)");
    println!("  --source PATH     explicit memory directory to scan");
    println!("  --dry-run         parse files + report, don't write records");
    println!("  -h, --help        this help");
    println!();
    println!("creates a namespace-per-project (claude.memory.<project-slug>) with");
    println!("schema claude.memory.<project-slug>.claudemd. Dedupes by content hash,");
    println!("so re-running is a no-op when files haven't changed.");
}

/// Minimal YAML-frontmatter parser for `key: value` pairs. Matches Claude
/// Code's `parseFrontmatter` semantics enough for memory files (which use
/// flat string values, no nesting). Returns (parsed_map, body_after_frontmatter).
///
/// Frontmatter format:
///   ---
///   key: value
///   other: another
///   ---
///   <body starts here>
fn parse_frontmatter(input: &str) -> (std::collections::HashMap<String, String>, String) {
    let mut map = std::collections::HashMap::new();
    let mut lines = input.lines();
    let first = match lines.next() {
        Some(l) => l.trim(),
        None => return (map, input.to_string()),
    };
    if first != "---" {
        // No frontmatter; whole file is body.
        return (map, input.to_string());
    }
    let mut body_start_line = 0usize;
    let mut found_end = false;
    for (i, line) in input.lines().enumerate().skip(1) {
        if line.trim() == "---" {
            body_start_line = i + 1;
            found_end = true;
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            let key = k.trim().to_string();
            let val = v.trim().trim_matches('"').trim_matches('\'').to_string();
            if !key.is_empty() {
                map.insert(key, val);
            }
        }
    }
    let body = if found_end {
        input
            .lines()
            .skip(body_start_line)
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        // Unterminated frontmatter — treat whole input as body to be safe.
        input.to_string()
    };
    (map, body)
}

/// Make a free-form string safe as a namespace segment: keep
/// [a-zA-Z0-9.-], replace everything else with `-`, collapse runs.
fn sanitize_ns_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_dash = false;
    for ch in s.chars() {
        let ok = ch.is_ascii_alphanumeric() || ch == '.' || ch == '-';
        if ok {
            out.push(ch);
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

// --- mcp / brain: exec sibling --------------------------------------------

fn cmd_exec_sibling(name: &str, args: &[String]) -> Result<i32, String> {
    let target = locate_sibling(name)?;
    let status: ExitStatus = Command::new(&target)
        .args(args)
        .status()
        .map_err(|e| format!("exec {}: {e}", target.display()))?;
    Ok(status.code().unwrap_or(1))
}

fn sibling_filenames(base: &str) -> Vec<String> {
    #[cfg(windows)]
    {
        vec![format!("{base}.exe"), base.to_string()]
    }
    #[cfg(not(windows))]
    {
        vec![base.to_string()]
    }
}

/// Find a sibling binary named `name`. Search order:
/// 1. Same directory as the current executable (plugin bundle case)
/// 2. `PATH` (`PATH`/`Path` split is OS-aware)
fn locate_sibling(name: &str) -> Result<PathBuf, String> {
    let names = sibling_filenames(name);
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for n in &names {
                let cand = dir.join(n);
                if cand.exists() {
                    return Ok(cand);
                }
            }
        }
    }
    if let Ok(path_var) = std::env::var("PATH") {
        for entry in std::env::split_paths(&path_var) {
            for n in &names {
                let cand = entry.join(n);
                if cand.exists() {
                    return Ok(cand);
                }
            }
        }
    }
    Err(format!(
        "couldn't locate `{name}`. expected it next to the `hellodb` binary \
         or on $PATH. if you're running from the workspace, try `cargo build --release` \
         first."
    ))
}

/// Register `hellodb-mcp` with OpenAI Codex (`codex mcp add`), if Codex CLI is installed.
fn cmd_integrate(args: &[String]) -> Result<i32, String> {
    match args.first().map(String::as_str) {
        Some("codex") => {
            let mcp = locate_sibling("hellodb-mcp")?;
            let exists = Command::new("codex")
                .args(["mcp", "get", "hellodb"])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            if exists {
                println!(
                    "Codex MCP server 'hellodb' already configured → {}",
                    mcp.display()
                );
                return Ok(0);
            }
            let status = Command::new("codex")
                .args(["mcp", "add", "hellodb", "--"])
                .arg(&mcp)
                .status()
                .map_err(|e| {
                    format!(
                        "could not run `codex`: {e}. install the OpenAI Codex CLI (https://developers.openai.com/codex/) and ensure it is on PATH."
                    )
                })?;
            if status.success() {
                println!("registered Codex MCP server 'hellodb' → {}", mcp.display());
                Ok(0)
            } else {
                Err(format!(
                    "`codex mcp add` exited with status {:?}. if this server already exists, try: codex mcp get hellodb",
                    status.code()
                ))
            }
        }
        _ => {
            eprintln!("usage: hellodb integrate codex");
            eprintln!();
            eprintln!(
                "Registers hellodb-mcp as a stdio MCP server in Codex (~/.codex/config.toml)."
            );
            eprintln!("Requires the `codex` CLI on PATH. Same as: codex mcp add hellodb -- <path-to-hellodb-mcp>");
            Ok(2)
        }
    }
}

// --- doctor ---------------------------------------------------------------

fn cmd_doctor() -> Result<i32, String> {
    let data = data_dir();
    let mut findings = Vec::new();
    let mut ok = true;

    // Data dir exists & writable
    if !data.exists() {
        findings.push((
            "data_dir_missing",
            format!("{} does not exist — run `hellodb init`", data.display()),
        ));
        ok = false;
    } else {
        let test = data.join(".doctor-write-test");
        match std::fs::write(&test, b"x") {
            Ok(_) => {
                let _ = std::fs::remove_file(&test);
            }
            Err(e) => {
                findings.push(("data_dir_not_writable", format!("{}: {e}", data.display())));
                ok = false;
            }
        }
    }

    // Identity permissions
    let identity = data.join("identity.key");
    if identity.exists() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&identity).unwrap().permissions().mode() & 0o777;
            if mode != 0o600 {
                findings.push((
                    "identity_perms_loose",
                    format!("identity.key mode is {mode:o}, expected 600"),
                ));
                ok = false;
            }
        }
    } else {
        findings.push(("identity_missing", "run `hellodb init`".into()));
        ok = false;
    }

    // Siblings findable
    for bin in &["hellodb-mcp", "hellodb-brain"] {
        if locate_sibling(bin).is_err() {
            findings.push((
                "sibling_missing",
                format!("{bin} not found near `hellodb` or on $PATH"),
            ));
            ok = false;
        }
    }

    // DB opens with derived key
    if identity.exists() {
        match load_identity(&identity) {
            Ok(kp) => {
                let db = data.join("local.db");
                if db.exists() {
                    let key = derive_sqlcipher_key(&kp.signing);
                    match path_as_str(&db) {
                        Ok(db_str) => {
                            if let Err(e) = SqliteEngine::open(db_str, &key) {
                                findings.push(("db_open_failed", format!("{e}")));
                                ok = false;
                            }
                        }
                        Err(e) => {
                            findings.push(("db_path_not_utf8", e));
                            ok = false;
                        }
                    }
                }
            }
            Err(e) => {
                findings.push(("identity_unreadable", e));
                ok = false;
            }
        }
    }

    println!(
        "{}",
        serde_json::json!({
            "ok": ok,
            "findings": findings
                .into_iter()
                .map(|(k, v)| serde_json::json!({ "code": k, "detail": v }))
                .collect::<Vec<_>>(),
        })
    );
    Ok(if ok { 0 } else { 1 })
}

// --- shared helpers --------------------------------------------------------

fn data_dir() -> PathBuf {
    if let Ok(override_) = std::env::var("HELLODB_HOME") {
        return PathBuf::from(override_);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".hellodb")
}

/// Borrow a `Path` as `&str`, returning a descriptive error if the path
/// is not valid UTF-8. rusqlite / SQLCipher require a `&str` path, so
/// we have to deal with this eventuality explicitly rather than panic.
///
/// Non-UTF-8 filesystem paths are rare on macOS/Linux but legal, and
/// someone setting HELLODB_HOME to a weird path shouldn't crash the
/// CLI with a `.unwrap()` panic.
fn path_as_str(p: &Path) -> Result<&str, String> {
    p.to_str()
        .ok_or_else(|| format!("path is not valid UTF-8: {}", p.display()))
}

fn load_identity(path: &Path) -> Result<KeyPair, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("reading {}: {e}", path.display()))?;
    if bytes.len() != 32 {
        return Err(format!(
            "identity.key must be 32 bytes, got {}",
            bytes.len()
        ));
    }
    let mut seed = [0u8; 32];
    seed.copy_from_slice(&bytes);
    let signing = SigningKey::from_bytes(&seed);
    let verifying = signing.verifying_key();
    Ok(KeyPair { signing, verifying })
}

/// MUST match hellodb-mcp's derive_sqlcipher_key (and hellodb-brain's copy of it).
fn derive_sqlcipher_key(signing: &SigningKey) -> String {
    let seed = signing.to_bytes();
    let mut input = Vec::with_capacity(64);
    input.extend_from_slice(b"hellodb-mcp-sqlcipher-v1:");
    input.extend_from_slice(&seed);
    content_hash(&input)
}
