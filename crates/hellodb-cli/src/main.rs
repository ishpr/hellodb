//! hellodb — unified CLI entry point.
//!
//! One command, clean subcommand surface:
//!
//!   hellodb init              — first-time setup (identity, brain.toml)
//!   hellodb status            — identity + namespaces + brain state
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

use hellodb_core::Record;
use hellodb_crypto::{content_hash, KeyPair, SigningKey};
use hellodb_storage::{SqliteEngine, StorageEngine};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(String::as_str) {
        Some("init") => cmd_init(&args[1..]),
        Some("status") => cmd_status(&args[1..]),
        Some("mcp") => cmd_exec_sibling("hellodb-mcp", &args[1..]),
        Some("brain") => cmd_exec_sibling("hellodb-brain", &args[1..]),
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
    println!("  mcp        run the MCP server (stdio transport; for Claude Code)");
    println!("  brain      run one passive-memory digest pass (see --help for flags)");
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
    let _ = SqliteEngine::open(db_path.to_str().unwrap(), &db_key)
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

[digest]
# MVP backend — deterministic, no LLM. See crates/hellodb-brain/src/digest.rs
# for how to wire a real LLM backend.
backend = "mock"
fact_schema_id = "brain.fact"
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
    let storage = SqliteEngine::open(db_path.to_str().unwrap(), &db_key)
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

// --- mcp / brain: exec sibling --------------------------------------------

fn cmd_exec_sibling(name: &str, args: &[String]) -> Result<i32, String> {
    let target = locate_sibling(name)?;
    let status: ExitStatus = Command::new(&target)
        .args(args)
        .status()
        .map_err(|e| format!("exec {}: {e}", target.display()))?;
    Ok(status.code().unwrap_or(1))
}

/// Find a sibling binary named `name`. Search order:
/// 1. Same directory as the current executable (plugin bundle case)
/// 2. `$PATH`
fn locate_sibling(name: &str) -> Result<PathBuf, String> {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let cand = dir.join(name);
            if cand.exists() {
                return Ok(cand);
            }
        }
    }
    // PATH fallback via `which`-style scan.
    if let Ok(path_var) = std::env::var("PATH") {
        for entry in path_var.split(':') {
            let cand = Path::new(entry).join(name);
            if cand.exists() {
                return Ok(cand);
            }
        }
    }
    Err(format!(
        "couldn't locate `{name}`. expected it next to the `hellodb` binary \
         or on $PATH. if you're running from the workspace, try `cargo build --release` \
         first."
    ))
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
                    if let Err(e) = SqliteEngine::open(db.to_str().unwrap(), &key) {
                        findings.push(("db_open_failed", format!("{e}")));
                        ok = false;
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
