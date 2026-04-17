//! Simple exclusive-creation file lock.
//!
//! `LockGuard::acquire` succeeds iff the lock file did not exist. Drop
//! removes the file. If a previous crash left a stale lock, delete it
//! manually (cheap to diagnose: the file contains the PID that held it).

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::BrainError;

pub struct LockGuard {
    path: PathBuf,
}

impl LockGuard {
    pub fn acquire(path: &Path) -> Result<Self, BrainError> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|e| {
                BrainError::Lock(format!(
                    "couldn't acquire lock at {}: {e}. \
                     if you're sure no other brain is running, delete the file.",
                    path.display()
                ))
            })?;
        writeln!(
            file,
            "pid={}\ntime_ms={}",
            std::process::id(),
            crate::now_ms()
        )?;
        Ok(Self {
            path: path.to_path_buf(),
        })
    }
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
