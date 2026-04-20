//! Sync manifest — tracks per-device, per-namespace sync state.
//!
//! Each device maintains a manifest that records what it has pushed
//! and pulled. Manifests are stored both locally (in StorageEngine as
//! system records) and remotely (in the SyncBackend) so other devices
//! can discover each other's sync progress.

use serde::{Deserialize, Serialize};

/// Sync state for one device in one namespace.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SyncManifest {
    /// The device this manifest belongs to.
    pub device_id: String,
    /// The namespace being synced.
    pub namespace: String,
    /// Timestamp of the most recently pushed record.
    pub last_push_cursor: u64,
    /// Timestamp of the most recently pulled delta.
    pub last_pull_cursor: u64,
    /// Tombstone IDs already pushed by this device.
    ///
    /// Branch changes do not currently carry per-change timestamps, so we keep
    /// a small manifest-side set of tombstones we've already emitted to avoid
    /// repeatedly re-sending historical deletions on every push.
    #[serde(default)]
    pub pushed_tombstones: Vec<String>,
    /// When this manifest was last updated.
    pub updated_at_ms: u64,
}

impl SyncManifest {
    /// Create a fresh manifest for a device + namespace (no sync history).
    pub fn new(device_id: impl Into<String>, namespace: impl Into<String>) -> Self {
        Self {
            device_id: device_id.into(),
            namespace: namespace.into(),
            last_push_cursor: 0,
            last_pull_cursor: 0,
            pushed_tombstones: Vec::new(),
            updated_at_ms: 0,
        }
    }
}

/// High-level sync status for a namespace.
#[derive(Debug)]
pub struct SyncStatus {
    pub namespace: String,
    pub last_push: Option<u64>,
    pub last_pull: Option<u64>,
    pub pending_push_count: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_serde_roundtrip() {
        let manifest = SyncManifest {
            device_id: "phone-a".into(),
            namespace: "commerce".into(),
            last_push_cursor: 5000,
            last_pull_cursor: 4500,
            pushed_tombstones: vec!["r1".into(), "r2".into()],
            updated_at_ms: 6000,
        };

        let json = serde_json::to_string(&manifest).unwrap();
        let restored: SyncManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(manifest, restored);
    }

    #[test]
    fn fresh_manifest_zeroed() {
        let m = SyncManifest::new("d1", "ns");
        assert_eq!(m.last_push_cursor, 0);
        assert_eq!(m.last_pull_cursor, 0);
        assert!(m.pushed_tombstones.is_empty());
        assert_eq!(m.updated_at_ms, 0);
    }
}
