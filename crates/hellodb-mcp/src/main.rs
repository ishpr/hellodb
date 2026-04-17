//! hellodb-mcp — Model Context Protocol server for hellodb.
//!
//! Transport: stdio (newline-delimited JSON-RPC 2.0).
//! Data: encrypted SQLCipher database under $HELLODB_HOME (default ~/.hellodb).
//! Identity: persistent Ed25519 key generated on first run.

mod identity;
mod protocol;
mod server;

use std::io::{self, BufRead, Write};

use hellodb_storage::SqliteEngine;

use crate::server::Server;

fn main() {
    if let Err(e) = run() {
        eprintln!("hellodb-mcp fatal: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let data_dir = identity::data_dir();
    let keypair =
        identity::load_or_create(&data_dir).map_err(|e| format!("failed to load identity: {e}"))?;

    let db_path = data_dir.join("local.db");
    let sqlcipher_key = identity::derive_sqlcipher_key(&keypair.signing);
    let storage = SqliteEngine::open(
        db_path.to_str().ok_or("db path is not valid utf-8")?,
        &sqlcipher_key,
    )
    .map_err(|e| format!("failed to open db at {}: {e}", db_path.display()))?;

    eprintln!(
        "hellodb-mcp ready | identity={} | db={}",
        keypair.verifying.fingerprint(),
        db_path.display()
    );

    let server = Server::new(storage, keypair);

    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                eprintln!("hellodb-mcp stdin read error: {e}");
                break;
            }
        };
        if line.trim().is_empty() {
            continue;
        }

        let req: protocol::Request = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("hellodb-mcp parse error: {e} | line={line}");
                continue;
            }
        };

        if let Some(resp) = server.handle(req) {
            let s = serde_json::to_string(&resp).map_err(|e| e.to_string())?;
            writeln!(out, "{s}").map_err(|e| e.to_string())?;
            out.flush().map_err(|e| e.to_string())?;
        }
    }

    Ok(())
}
