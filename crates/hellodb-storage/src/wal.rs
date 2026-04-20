//! Write-ahead log for crash recovery.
//!
//! This is a standalone Phase 0 WAL utility module. The current SqliteEngine
//! does not yet route mutations through this WAL automatically.
//!
//! Phase 0 implementation: simplified append-only JSON lines file.

use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};

use crate::error::StorageError;

/// A WAL entry representing a single mutation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WalEntry {
    PutRecord { record_json: String, branch: String },
    DeleteRecord { record_id: String, branch: String },
    CreateNamespace { namespace_json: String },
    CreateBranch { branch_json: String },
    MergeBranch { branch_id: String },
    RegisterSchema { schema_json: String },
}

/// WAL transaction: a batch of entries that must be applied atomically.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalTransaction {
    pub id: u64,
    pub entries: Vec<WalEntry>,
    pub committed: bool,
}

/// Write-ahead log backed by a file.
pub struct Wal {
    /// Path to the WAL file.
    path: String,
    /// Current transaction counter.
    next_tx_id: u64,
}

impl Wal {
    /// Open or create a WAL file.
    pub fn open(path: &str) -> Result<Self, StorageError> {
        // Read existing entries to find the next tx_id
        let next_tx_id = if std::path::Path::new(path).exists() {
            let file = File::open(path).map_err(StorageError::Io)?;
            let reader = BufReader::new(file);
            let mut max_id = 0u64;
            for line in reader.lines() {
                let line = line.map_err(StorageError::Io)?;
                if let Ok(tx) = serde_json::from_str::<WalTransaction>(&line) {
                    if tx.id > max_id {
                        max_id = tx.id;
                    }
                }
            }
            max_id + 1
        } else {
            0
        };

        Ok(Self {
            path: path.to_string(),
            next_tx_id,
        })
    }

    /// Begin a new transaction.
    pub fn begin(&mut self) -> WalTransaction {
        let tx = WalTransaction {
            id: self.next_tx_id,
            entries: Vec::new(),
            committed: false,
        };
        self.next_tx_id += 1;
        tx
    }

    /// Append an entry to a transaction (not yet committed).
    pub fn append(&mut self, tx: &mut WalTransaction, entry: WalEntry) -> Result<(), StorageError> {
        tx.entries.push(entry);
        Ok(())
    }

    /// Commit a transaction (write to disk and fsync).
    pub fn commit(&mut self, tx: &mut WalTransaction) -> Result<(), StorageError> {
        tx.committed = true;
        let line = serde_json::to_string(tx).map_err(StorageError::Serialization)?;

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(StorageError::Io)?;

        writeln!(file, "{}", line).map_err(StorageError::Io)?;
        file.sync_all().map_err(StorageError::Io)?;

        Ok(())
    }

    /// Read all committed transactions (for replay on startup).
    pub fn read_committed(&self) -> Result<Vec<WalTransaction>, StorageError> {
        if !std::path::Path::new(&self.path).exists() {
            return Ok(Vec::new());
        }

        let file = File::open(&self.path).map_err(StorageError::Io)?;
        let reader = BufReader::new(file);
        let mut transactions = Vec::new();

        for line in reader.lines() {
            let line = line.map_err(StorageError::Io)?;
            if let Ok(tx) = serde_json::from_str::<WalTransaction>(&line) {
                if tx.committed {
                    transactions.push(tx);
                }
            }
        }

        Ok(transactions)
    }

    /// Truncate the WAL after successful application.
    pub fn truncate(&mut self) -> Result<(), StorageError> {
        File::create(&self.path).map_err(StorageError::Io)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wal_write_and_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.wal");
        let path_str = path.to_str().unwrap();

        let mut wal = Wal::open(path_str).unwrap();

        // Write a transaction
        let mut tx = wal.begin();
        wal.append(
            &mut tx,
            WalEntry::CreateNamespace {
                namespace_json: r#"{"id":"test"}"#.into(),
            },
        )
        .unwrap();
        wal.append(
            &mut tx,
            WalEntry::PutRecord {
                record_json: r#"{"record_id":"r1"}"#.into(),
                branch: "test/main".into(),
            },
        )
        .unwrap();
        wal.commit(&mut tx).unwrap();

        // Read back
        let committed = wal.read_committed().unwrap();
        assert_eq!(committed.len(), 1);
        assert_eq!(committed[0].entries.len(), 2);
        assert!(committed[0].committed);
    }

    #[test]
    fn wal_truncate() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.wal");
        let path_str = path.to_str().unwrap();

        let mut wal = Wal::open(path_str).unwrap();
        let mut tx = wal.begin();
        wal.append(
            &mut tx,
            WalEntry::CreateNamespace {
                namespace_json: "{}".into(),
            },
        )
        .unwrap();
        wal.commit(&mut tx).unwrap();

        wal.truncate().unwrap();

        let committed = wal.read_committed().unwrap();
        assert_eq!(committed.len(), 0);
    }

    #[test]
    fn wal_reopen_continues_tx_ids() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.wal");
        let path_str = path.to_str().unwrap();

        {
            let mut wal = Wal::open(path_str).unwrap();
            let mut tx = wal.begin();
            assert_eq!(tx.id, 0);
            wal.commit(&mut tx).unwrap();
        }

        {
            let mut wal = Wal::open(path_str).unwrap();
            let tx = wal.begin();
            assert_eq!(tx.id, 1);
        }
    }
}
