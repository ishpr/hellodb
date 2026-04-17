//! Exclusive-creation file lock for the brain pass.
//!
//! `LockGuard::acquire` succeeds iff no other brain instance is running.
//! Drop removes the file so the next run can acquire.
//!
//! Stale-lock handling: a crash leaves the file behind and blocks every
//! subsequent run. To avoid silent stuck-forever state, we read the PID
//! the lockfile recorded on acquire failure and probe liveness (POSIX
//! `kill(pid, 0)`). If that process doesn't exist, we treat the lockfile
//! as stale, remove it, and retry acquire once.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::BrainError;

pub struct LockGuard {
    path: PathBuf,
}

impl LockGuard {
    pub fn acquire(path: &Path) -> Result<Self, BrainError> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        match Self::try_create(path) {
            Ok(g) => Ok(g),
            Err(first_err) => {
                // On failure, check if the holder is actually alive. If not,
                // remove the stale lockfile and retry once.
                if let Some(pid) = Self::read_pid(path) {
                    if !Self::process_alive(pid) {
                        eprintln!(
                            "brain: removing stale lockfile from dead pid {pid} at {}",
                            path.display()
                        );
                        let _ = fs::remove_file(path);
                        return Self::try_create(path);
                    }
                }
                Err(first_err)
            }
        }
    }

    fn try_create(path: &Path) -> Result<Self, BrainError> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|e| {
                BrainError::Lock(format!(
                    "couldn't acquire lock at {}: {e}. \
                     another brain is running, or the lockfile was left by a previous crash. \
                     if you're sure none are running, delete the file manually.",
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

    fn read_pid(path: &Path) -> Option<u32> {
        let s = fs::read_to_string(path).ok()?;
        for line in s.lines() {
            if let Some(rest) = line.strip_prefix("pid=") {
                return rest.trim().parse().ok();
            }
        }
        None
    }

    /// Unix-only liveness probe via `kill(pid, 0)`. Returns true if we *know*
    /// the process exists; on error or permission-denied, returns true (safer
    /// to assume alive than to clobber an active lock). On non-Unix always
    /// returns true (no probe available — users can delete the file manually).
    #[cfg(unix)]
    fn process_alive(pid: u32) -> bool {
        // SAFETY: `kill(pid, 0)` is a pure signal-probe with no side effects
        // when signal is 0. Returns 0 if the process exists OR exists but we
        // lack permission to signal it; -1 with ESRCH if the process is gone.
        // We treat any non-ESRCH result as "alive" to avoid false positives.
        let ret = unsafe { libc_kill(pid as i32, 0) };
        if ret == 0 {
            return true;
        }
        // errno inspection would require an extra dep. std::io::Error::last_os_error
        // is good enough.
        let err = std::io::Error::last_os_error();
        // ESRCH = 3 on every Unix we support (macOS, Linux, *BSD).
        err.raw_os_error() != Some(3)
    }

    #[cfg(not(unix))]
    fn process_alive(_pid: u32) -> bool {
        // No probe on non-Unix. Users can delete the lockfile manually.
        true
    }
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

// Minimal FFI binding so we don't pull in the `libc` crate just for this.
#[cfg(unix)]
extern "C" {
    #[link_name = "kill"]
    fn libc_kill(pid: i32, sig: i32) -> i32;
}
