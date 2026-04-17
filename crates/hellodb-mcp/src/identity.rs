//! Persistent Ed25519 identity for the MCP server.
//!
//! The identity is this machine's "agent key" — it owns namespaces it
//! creates and signs every record it writes. Persisted as raw 32 bytes
//! at `$HELLODB_HOME/identity.key` (default `~/.hellodb/identity.key`).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use hellodb_crypto::{
    content_hash, content_hash_bytes, KeyPair, MasterKey, NamespaceKey, SigningKey,
};

pub fn data_dir() -> PathBuf {
    if let Ok(override_) = std::env::var("HELLODB_HOME") {
        return PathBuf::from(override_);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".hellodb")
}

pub fn load_or_create(dir: &Path) -> io::Result<KeyPair> {
    fs::create_dir_all(dir)?;
    let path = dir.join("identity.key");

    if path.exists() {
        let bytes = fs::read(&path)?;
        if bytes.len() != 32 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("identity.key must be 32 bytes, got {}", bytes.len()),
            ));
        }
        let mut seed = [0u8; 32];
        seed.copy_from_slice(&bytes);
        let signing = SigningKey::from_bytes(&seed);
        let verifying = signing.verifying_key();
        Ok(KeyPair { signing, verifying })
    } else {
        let kp = KeyPair::generate();
        let seed = kp.signing.to_bytes();
        fs::write(&path, seed)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&path)?.permissions();
            perms.set_mode(0o600);
            fs::set_permissions(&path, perms)?;
        }
        Ok(kp)
    }
}

/// Deterministically derive the SQLCipher key from the identity seed.
/// Stable across restarts as long as the identity.key is stable.
pub fn derive_sqlcipher_key(signing: &SigningKey) -> String {
    let seed = signing.to_bytes();
    let mut input = Vec::with_capacity(64);
    input.extend_from_slice(b"hellodb-mcp-sqlcipher-v1:");
    input.extend_from_slice(&seed);
    content_hash(&input)
}

/// Deterministically derive a per-namespace vector-index key from the
/// identity seed. Stable across restarts as long as the identity.key is
/// stable, and distinct per namespace so compromise of one namespace's
/// vector file doesn't expose others.
///
/// The derivation is layered: we BLAKE3-hash a versioned context string
/// together with the namespace and the identity seed to produce a 32-byte
/// "master" seed, then run it through [`MasterKey::derive_namespace_key`]
/// so the final key is produced by the same KDF path used for record
/// encryption. This keeps the namespace-separation property even if
/// someone later extracts the intermediate seed.
pub fn derive_namespace_vector_key(signing: &SigningKey, namespace: &str) -> NamespaceKey {
    let seed = signing.to_bytes();
    let mut input =
        Vec::with_capacity(64 + namespace.len() + "hellodb-mcp-vector-key-v1:".len() + 1);
    input.extend_from_slice(b"hellodb-mcp-vector-key-v1:");
    input.extend_from_slice(namespace.as_bytes());
    input.extend_from_slice(b":");
    input.extend_from_slice(&seed);
    let derived_seed = content_hash_bytes(&input);
    MasterKey::from_bytes(derived_seed).derive_namespace_key(namespace)
}
