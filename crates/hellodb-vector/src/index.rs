//! Flat, per-namespace vector index sealed at rest with a [`NamespaceKey`].
//!
//! On-disk format: a [`SealedBox`] whose plaintext is the JSON serialization of
//! [`IndexFile`] (version + dim + entries). JSON was chosen over bincode for
//! debuggability — the index is small and the overhead is negligible at
//! personal-memory scale.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use hellodb_crypto::encryption::{open_with_key, seal_with_key};
use hellodb_crypto::{NamespaceKey, SealedBox};
use serde::{Deserialize, Serialize};

use crate::error::VectorError;
use crate::math;

/// Current on-disk format version. Bump if the file layout changes.
const FORMAT_VERSION: u32 = 1;

/// A single search result.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    pub record_id: String,
    /// Cosine similarity in `[-1.0, 1.0]`. Higher is more similar.
    pub score: f32,
}

/// The plaintext on-disk payload for a namespace's vector index.
#[derive(Debug, Serialize, Deserialize)]
struct IndexFile {
    version: u32,
    /// Dimensionality of every stored vector. `None` when empty.
    dim: Option<usize>,
    /// All entries, with embeddings already L2-normalized.
    entries: Vec<Entry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    record_id: String,
    /// Stored as a unit vector so cosine similarity reduces to a dot product.
    embedding: Vec<f32>,
}

/// Per-namespace vector index. Persisted to `{dir}/{namespace}.vec`.
///
/// **Concurrency:** each `open()` acquires a POSIX advisory `flock` on a
/// sidecar `.vec.lock` file and holds it for the VectorIndex's lifetime.
/// This serializes open+mutate+flush across processes so two MCP tool
/// calls doing `hellodb_upsert_embedding` on the same namespace can't
/// silently lose each other's writes (read-same-state → each mutates →
/// each flushes → last-write-wins). On non-Unix platforms the lock is
/// a no-op — users on those platforms shouldn't run concurrent writers
/// to the same namespace yet.
pub struct VectorIndex {
    path: PathBuf,
    key_bytes: [u8; 32],
    namespace: String,
    dim: Option<usize>,
    /// record_id -> position in `entries`, for O(1) upsert/remove.
    index: HashMap<String, usize>,
    entries: Vec<Entry>,
    /// Held for the lifetime of the VectorIndex; dropped when the struct
    /// is dropped or (on crash) when the process exits.
    #[allow(dead_code)]
    _lock: FileLock,
}

impl std::fmt::Debug for VectorIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Deliberately omit `key_bytes` so the key material is never leaked
        // via Debug output (e.g. into panic messages or logs).
        f.debug_struct("VectorIndex")
            .field("path", &self.path)
            .field("namespace", &self.namespace)
            .field("dim", &self.dim)
            .field("len", &self.entries.len())
            .finish()
    }
}

impl VectorIndex {
    /// Open or create the vector index for a namespace.
    ///
    /// If `{dir}/{namespace}.vec` does not exist, returns an empty index —
    /// nothing is written to disk until the first mutation or explicit
    /// [`flush`](Self::flush) call.
    ///
    /// If the file exists but cannot be decrypted with `key`, returns
    /// [`VectorError::Crypto`].
    pub fn open(dir: &Path, namespace: &str, key: &NamespaceKey) -> Result<Self, VectorError> {
        fs::create_dir_all(dir)?;
        let path = dir.join(format!("{}.vec", namespace));
        let lock_path = dir.join(format!("{}.vec.lock", namespace));
        let key_bytes = key.to_bytes();

        // Acquire the exclusive file lock BEFORE reading state. Any concurrent
        // writer is serialized behind us (or us behind them). The lock is
        // advisory, not mandatory — nothing stops a caller who bypasses
        // VectorIndex from writing the .vec directly, but we own all write
        // paths in the workspace.
        let lock = FileLock::acquire_exclusive(&lock_path)?;

        if !path.exists() {
            return Ok(Self {
                path,
                key_bytes,
                namespace: namespace.to_string(),
                dim: None,
                index: HashMap::new(),
                entries: Vec::new(),
                _lock: lock,
            });
        }

        let sealed_bytes = fs::read(&path)?;
        let sealed: SealedBox = serde_json::from_slice(&sealed_bytes)?;
        let plaintext = open_with_key(&key_bytes, &sealed)?;
        let file: IndexFile = serde_json::from_slice(&plaintext)?;

        let mut index = HashMap::with_capacity(file.entries.len());
        for (i, e) in file.entries.iter().enumerate() {
            index.insert(e.record_id.clone(), i);
        }

        Ok(Self {
            path,
            key_bytes,
            namespace: namespace.to_string(),
            dim: file.dim,
            index,
            entries: file.entries,
            _lock: lock,
        })
    }

    /// Number of vectors currently indexed.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The namespace this index belongs to.
    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    /// Insert or overwrite the embedding for `record_id`.
    ///
    /// The first upsert pins the dimensionality of the index; subsequent
    /// upserts with a different length return [`VectorError::DimensionMismatch`].
    /// Zero-length vectors are rejected with [`VectorError::InvalidEmbedding`].
    ///
    /// The embedding is L2-normalized before being stored, so search can
    /// treat cosine similarity as a dot product.
    ///
    /// Persists to disk before returning.
    pub fn upsert(
        &mut self,
        record_id: String,
        mut embedding: Vec<f32>,
    ) -> Result<(), VectorError> {
        // Validate finiteness first — NaN/inf would poison ordering.
        if embedding.iter().any(|x| !x.is_finite()) {
            return Err(VectorError::InvalidEmbedding(
                "embedding contains NaN or infinity".into(),
            ));
        }

        if let Some(expected) = self.dim {
            if embedding.len() != expected {
                return Err(VectorError::DimensionMismatch {
                    expected,
                    got: embedding.len(),
                });
            }
        } else {
            if embedding.is_empty() {
                return Err(VectorError::InvalidEmbedding(
                    "embedding must be non-empty".into(),
                ));
            }
            self.dim = Some(embedding.len());
        }

        let norm = math::normalize(&mut embedding);
        if !(norm > 0.0 && norm.is_finite()) {
            return Err(VectorError::InvalidEmbedding(
                "embedding has zero length".into(),
            ));
        }

        match self.index.get(&record_id).copied() {
            Some(i) => {
                self.entries[i].embedding = embedding;
            }
            None => {
                let i = self.entries.len();
                self.entries.push(Entry {
                    record_id: record_id.clone(),
                    embedding,
                });
                self.index.insert(record_id, i);
            }
        }

        self.flush()
    }

    /// Remove `record_id`. No-op if it isn't indexed.
    ///
    /// Persists to disk before returning if an entry was actually removed.
    pub fn remove(&mut self, record_id: &str) -> Result<(), VectorError> {
        let Some(i) = self.index.remove(record_id) else {
            return Ok(());
        };
        // swap_remove keeps entries contiguous but shifts the last element's index.
        self.entries.swap_remove(i);
        if i < self.entries.len() {
            let moved_id = self.entries[i].record_id.clone();
            self.index.insert(moved_id, i);
        }
        if self.entries.is_empty() {
            self.dim = None;
        }
        self.flush()
    }

    /// Top-`top_k` nearest neighbors by cosine similarity, highest score first.
    ///
    /// Returns an empty vector when the index is empty or `top_k == 0`.
    /// Returns [`VectorError::DimensionMismatch`] if `query.len()` differs
    /// from the index dimensionality.
    pub fn search(&self, query: &[f32], top_k: usize) -> Result<Vec<SearchHit>, VectorError> {
        if top_k == 0 || self.entries.is_empty() {
            return Ok(Vec::new());
        }

        let expected = self.dim.expect("dim set when entries exist");
        if query.len() != expected {
            return Err(VectorError::DimensionMismatch {
                expected,
                got: query.len(),
            });
        }
        if query.iter().any(|x| !x.is_finite()) {
            return Err(VectorError::InvalidEmbedding(
                "query contains NaN or infinity".into(),
            ));
        }

        // Normalize the query so similarity is a dot product against stored unit vectors.
        let mut q = query.to_vec();
        let qn = math::normalize(&mut q);
        if !(qn > 0.0 && qn.is_finite()) {
            return Err(VectorError::InvalidEmbedding(
                "query has zero length".into(),
            ));
        }

        // TODO: HNSW. For personal-memory scale (thousands), a flat scan is fine.
        let mut scored: Vec<SearchHit> = self
            .entries
            .iter()
            .map(|e| SearchHit {
                record_id: e.record_id.clone(),
                score: math::dot(&q, &e.embedding),
            })
            .collect();

        // Descending by score. Use partial_cmp; inputs are finite so no NaNs.
        scored.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        scored.truncate(top_k);
        Ok(scored)
    }

    /// Serialize, encrypt, and atomically persist the index.
    ///
    /// Writes to `{path}.tmp` then renames over the target. On platforms that
    /// support atomic rename within a directory, readers never observe a
    /// half-written file.
    pub fn flush(&self) -> Result<(), VectorError> {
        let file = IndexFile {
            version: FORMAT_VERSION,
            dim: self.dim,
            entries: self.entries.clone(),
        };
        let plaintext = serde_json::to_vec(&file)?;
        let sealed = seal_with_key(&self.key_bytes, &plaintext);
        let sealed_bytes = serde_json::to_vec(&sealed)?;

        if let Some(parent) = self.path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }

        let tmp = self.path.with_extension("vec.tmp");
        fs::write(&tmp, &sealed_bytes)?;
        fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}

// --- File locking --------------------------------------------------------
//
// POSIX `flock(fd, LOCK_EX)` held for the lifetime of the VectorIndex.
// When the FileLock is dropped (or the process exits), the kernel releases
// the lock automatically. Non-Unix: no-op.

pub(crate) struct FileLock {
    #[cfg(unix)]
    _fd: std::fs::File,
}

impl FileLock {
    pub(crate) fn acquire_exclusive(path: &Path) -> Result<Self, VectorError> {
        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;
            let f = fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(false)
                .open(path)?;
            // LOCK_EX = 2 on every Unix we care about (Linux, macOS, *BSD).
            // Blocks until the lock is available. flock(2) returns 0 on
            // success, -1 on error.
            let ret = unsafe { libc_flock(f.as_raw_fd(), 2) };
            if ret != 0 {
                let err = std::io::Error::last_os_error();
                return Err(VectorError::Io(err));
            }
            Ok(FileLock { _fd: f })
        }
        #[cfg(not(unix))]
        {
            // No advisory-lock facility without LockFileEx+more FFI; leave
            // as a documented no-op. Users running multiple concurrent
            // writers on non-Unix should serialize externally until we add
            // platform-specific locking.
            let _ = path;
            Ok(FileLock {})
        }
    }
}

#[cfg(unix)]
extern "C" {
    #[link_name = "flock"]
    fn libc_flock(fd: i32, operation: i32) -> i32;
}

#[cfg(test)]
mod tests {
    use super::*;
    use hellodb_crypto::MasterKey;
    use tempfile::TempDir;

    fn make_key(namespace: &str) -> NamespaceKey {
        MasterKey::from_bytes([7u8; 32]).derive_namespace_key(namespace)
    }

    #[test]
    fn upsert_then_search() {
        let dir = TempDir::new().unwrap();
        let key = make_key("test.recall");
        let mut idx = VectorIndex::open(dir.path(), "test.recall", &key).unwrap();

        idx.upsert("a".into(), vec![1.0, 0.0, 0.0]).unwrap();
        idx.upsert("b".into(), vec![0.0, 1.0, 0.0]).unwrap();
        idx.upsert("c".into(), vec![0.9, 0.1, 0.0]).unwrap();

        assert_eq!(idx.len(), 3);
        assert!(!idx.is_empty());

        let hits = idx.search(&[1.0, 0.0, 0.0], 3).unwrap();
        assert_eq!(hits.len(), 3);
        // Closest to [1,0,0] should be "a", then "c", then "b".
        assert_eq!(hits[0].record_id, "a");
        assert_eq!(hits[1].record_id, "c");
        assert_eq!(hits[2].record_id, "b");
        // Scores must be monotonically non-increasing.
        assert!(hits[0].score >= hits[1].score);
        assert!(hits[1].score >= hits[2].score);
        // Exact match should score ~1.0.
        assert!((hits[0].score - 1.0).abs() < 1e-5);
    }

    #[test]
    fn upsert_overwrites() {
        let dir = TempDir::new().unwrap();
        let key = make_key("test.overwrite");
        let mut idx = VectorIndex::open(dir.path(), "test.overwrite", &key).unwrap();

        idx.upsert("a".into(), vec![1.0, 0.0, 0.0]).unwrap();
        idx.upsert("a".into(), vec![0.0, 1.0, 0.0]).unwrap();

        assert_eq!(idx.len(), 1);

        let hits = idx.search(&[0.0, 1.0, 0.0], 5).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].record_id, "a");
        assert!((hits[0].score - 1.0).abs() < 1e-5);
    }

    #[test]
    fn dimension_mismatch_rejected() {
        let dir = TempDir::new().unwrap();
        let key = make_key("test.dim");
        let mut idx = VectorIndex::open(dir.path(), "test.dim", &key).unwrap();

        idx.upsert("a".into(), vec![1.0, 0.0, 0.0, 0.0]).unwrap();

        let err = idx
            .upsert("b".into(), vec![1.0, 0.0, 0.0, 0.0, 0.0])
            .unwrap_err();
        match err {
            VectorError::DimensionMismatch { expected, got } => {
                assert_eq!(expected, 4);
                assert_eq!(got, 5);
            }
            other => panic!("expected DimensionMismatch, got {other:?}"),
        }

        // Search dim must match too.
        let err = idx.search(&[1.0, 0.0, 0.0], 1).unwrap_err();
        assert!(matches!(err, VectorError::DimensionMismatch { .. }));
    }

    #[test]
    fn zero_vector_rejected() {
        let dir = TempDir::new().unwrap();
        let key = make_key("test.zero");
        let mut idx = VectorIndex::open(dir.path(), "test.zero", &key).unwrap();

        let err = idx.upsert("a".into(), vec![0.0, 0.0, 0.0]).unwrap_err();
        assert!(matches!(err, VectorError::InvalidEmbedding(_)));
        assert_eq!(idx.len(), 0);

        // After a successful upsert pins the dim, an all-zero vector of the
        // correct length is still rejected.
        idx.upsert("a".into(), vec![1.0, 0.0, 0.0]).unwrap();
        let err = idx.upsert("b".into(), vec![0.0, 0.0, 0.0]).unwrap_err();
        assert!(matches!(err, VectorError::InvalidEmbedding(_)));
    }

    #[test]
    fn remove_works() {
        let dir = TempDir::new().unwrap();
        let key = make_key("test.remove");
        let mut idx = VectorIndex::open(dir.path(), "test.remove", &key).unwrap();

        idx.upsert("a".into(), vec![1.0, 0.0]).unwrap();
        idx.upsert("b".into(), vec![0.0, 1.0]).unwrap();
        assert_eq!(idx.len(), 2);

        idx.remove("a").unwrap();
        assert_eq!(idx.len(), 1);

        let hits = idx.search(&[1.0, 0.0], 5).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].record_id, "b");

        idx.remove("b").unwrap();
        assert!(idx.is_empty());
        let hits = idx.search(&[1.0, 0.0], 5).unwrap();
        assert!(hits.is_empty());

        // Removing a missing id is a silent no-op.
        idx.remove("ghost").unwrap();
    }

    #[test]
    fn persistence_roundtrip() {
        let dir = TempDir::new().unwrap();
        let key = make_key("test.persist");

        {
            let mut idx = VectorIndex::open(dir.path(), "test.persist", &key).unwrap();
            idx.upsert("a".into(), vec![1.0, 0.0, 0.0]).unwrap();
            idx.upsert("b".into(), vec![0.0, 1.0, 0.0]).unwrap();
            idx.flush().unwrap();
        }

        let idx = VectorIndex::open(dir.path(), "test.persist", &key).unwrap();
        assert_eq!(idx.len(), 2);
        let hits = idx.search(&[1.0, 0.0, 0.0], 2).unwrap();
        assert_eq!(hits[0].record_id, "a");
        assert_eq!(hits[1].record_id, "b");
    }

    #[test]
    fn wrong_key_fails() {
        let dir = TempDir::new().unwrap();
        let key_good = make_key("test.wrongkey");
        let key_bad = MasterKey::from_bytes([99u8; 32]).derive_namespace_key("test.wrongkey");

        {
            let mut idx = VectorIndex::open(dir.path(), "test.wrongkey", &key_good).unwrap();
            idx.upsert("a".into(), vec![1.0, 0.0]).unwrap();
        }

        let err = VectorIndex::open(dir.path(), "test.wrongkey", &key_bad).unwrap_err();
        assert!(
            matches!(err, VectorError::Crypto(_)),
            "expected Crypto, got {err:?}"
        );
    }

    #[test]
    fn opens_empty_when_file_missing() {
        let dir = TempDir::new().unwrap();
        let key = make_key("test.missing");
        let idx = VectorIndex::open(dir.path(), "test.missing", &key).unwrap();
        assert!(idx.is_empty());
        assert_eq!(idx.len(), 0);
    }

    #[test]
    fn search_on_empty_returns_empty() {
        let dir = TempDir::new().unwrap();
        let key = make_key("test.empty");
        let idx = VectorIndex::open(dir.path(), "test.empty", &key).unwrap();
        let hits = idx.search(&[1.0, 0.0], 5).unwrap();
        assert!(hits.is_empty());
    }
}
