//! Sync engine — orchestrates push/pull/reconcile.
//!
//! The SyncEngine is the "inverse ingestion pipeline": instead of a cloud
//! service pulling data in, YOUR device pushes encrypted deltas to YOUR
//! personal cloud bucket, and other devices pull and merge.

use hellodb_core::Record;
use hellodb_crypto::NamespaceKey;
use hellodb_storage::StorageEngine;
use std::collections::HashSet;

use crate::backend::SyncBackend;
use crate::conflict::{resolve_conflict, ConflictStrategy, SyncConflict};
use crate::delta::{open_delta, seal_delta, DeltaBundle, SealedDelta};
use crate::error::SyncError;
use crate::manifest::{SyncManifest, SyncStatus};

/// Result of a push operation.
#[derive(Debug)]
pub struct PushResult {
    /// Number of records included in the delta.
    pub records_pushed: usize,
    /// Number of tombstones included.
    pub tombstones_pushed: usize,
    /// The blob key in the backend where the delta was stored.
    pub delta_key: String,
}

/// Result of a pull operation.
#[derive(Debug)]
pub struct PullResult {
    /// Number of new records merged into local storage.
    pub records_merged: usize,
    /// Conflicts detected and resolved.
    pub conflicts: Vec<SyncConflict>,
    /// Number of remote deltas applied.
    pub deltas_applied: usize,
}

/// The sync engine. Wraps a StorageEngine and device identity to
/// orchestrate encrypted delta sync to a personal cloud backend.
pub struct SyncEngine<'a> {
    storage: &'a mut dyn StorageEngine,
    device_id: String,
}

impl<'a> SyncEngine<'a> {
    /// Create a new sync engine for the given device.
    pub fn new(storage: &'a mut dyn StorageEngine, device_id: impl Into<String>) -> Self {
        Self {
            storage,
            device_id: device_id.into(),
        }
    }

    /// Push local changes to the cloud backend.
    ///
    /// 1. Load or create manifest for this device+namespace
    /// 2. Query records with created_at_ms > last_push_cursor
    /// 3. Include branch tombstones not yet pushed
    /// 4. Bundle into a DeltaBundle, encrypt with ns_key
    /// 5. Upload encrypted delta to backend
    /// 6. Update manifest
    pub fn push(
        &mut self,
        namespace: &str,
        branch: &str,
        ns_key: &NamespaceKey,
        backend: &mut dyn SyncBackend,
        now_ms: u64,
    ) -> Result<PushResult, SyncError> {
        let mut manifest = self.load_or_create_manifest(namespace, backend)?;

        // Fetch records newer than last push
        let all_records =
            self.storage
                .list_records_by_namespace(namespace, branch, usize::MAX, 0)?;

        let new_records: Vec<Record> = all_records
            .into_iter()
            .filter(|r| r.created_at_ms > manifest.last_push_cursor)
            .collect();

        let branch_state = self
            .storage
            .get_branch(branch)?
            .ok_or_else(|| SyncError::Conflict(format!("branch not found: {branch}")))?;
        let already_pushed: HashSet<String> = manifest.pushed_tombstones.iter().cloned().collect();
        let new_tombstones: Vec<String> = branch_state
            .changes
            .iter()
            .filter(|(_, is_present)| !**is_present)
            .map(|(record_id, _)| record_id.clone())
            .filter(|record_id| !already_pushed.contains(record_id))
            .collect();

        if new_records.is_empty() && new_tombstones.is_empty() {
            return Ok(PushResult {
                records_pushed: 0,
                tombstones_pushed: 0,
                delta_key: String::new(),
            });
        }

        let to_cursor = if new_records.is_empty() {
            // Tombstone-only push: advance cursor explicitly so this delta
            // gets a unique key and downstream pull cursors can progress.
            std::cmp::max(manifest.last_push_cursor.saturating_add(1), now_ms)
        } else {
            new_records
                .iter()
                .map(|r| r.created_at_ms)
                .max()
                .unwrap_or(manifest.last_push_cursor)
        };

        let record_count = new_records.len();

        let bundle = DeltaBundle {
            device_id: self.device_id.clone(),
            namespace: namespace.into(),
            branch: branch.into(),
            from_cursor: manifest.last_push_cursor,
            to_cursor,
            records: new_records,
            tombstones: new_tombstones.clone(),
            created_at_ms: now_ms,
        };

        // Encrypt and upload
        let sealed = seal_delta(&bundle, ns_key)?;
        let delta_key = format!(
            "{}/deltas/{}/{}.delta",
            namespace, self.device_id, to_cursor
        );
        let delta_bytes = serde_json::to_vec(&sealed)?;
        backend.put_blob(&delta_key, &delta_bytes)?;

        // Update manifest
        manifest.last_push_cursor = to_cursor;
        manifest
            .pushed_tombstones
            .extend(new_tombstones.iter().cloned());
        manifest.pushed_tombstones.sort();
        manifest.pushed_tombstones.dedup();
        manifest.updated_at_ms = now_ms;
        self.save_manifest(&manifest, backend)?;

        Ok(PushResult {
            records_pushed: record_count,
            tombstones_pushed: new_tombstones.len(),
            delta_key,
        })
    }

    /// Pull remote changes from the cloud backend.
    ///
    /// 1. Load manifest
    /// 2. List remote deltas from OTHER devices
    /// 3. Download, decrypt, and merge each delta
    /// 4. Detect and resolve conflicts
    /// 5. Update manifest
    pub fn pull(
        &mut self,
        namespace: &str,
        branch: &str,
        ns_key: &NamespaceKey,
        backend: &mut dyn SyncBackend,
        strategy: ConflictStrategy,
        now_ms: u64,
    ) -> Result<PullResult, SyncError> {
        let mut manifest = self.load_or_create_manifest(namespace, backend)?;

        // List all deltas in this namespace
        let prefix = format!("{}/deltas/", namespace);
        let all_keys = backend.list_blobs(&prefix)?;

        // Filter to deltas from OTHER devices
        let my_prefix = format!("{}/deltas/{}/", namespace, self.device_id);
        let remote_keys: Vec<String> = all_keys
            .into_iter()
            .filter(|k| !k.starts_with(&my_prefix))
            .collect();

        struct PendingDelta {
            key: String,
            sealed: SealedDelta,
        }
        let mut pending = Vec::new();
        for key in remote_keys {
            let blob = match backend.get_blob(&key)? {
                Some(b) => b,
                None => continue,
            };
            let sealed: SealedDelta = serde_json::from_slice(&blob)?;
            if sealed.metadata.to_cursor <= manifest.last_pull_cursor {
                continue;
            }
            pending.push(PendingDelta { key, sealed });
        }
        // Deterministic replay order across backends and devices.
        pending.sort_by(|a, b| {
            let a_cursor = a.sealed.metadata.to_cursor;
            let b_cursor = b.sealed.metadata.to_cursor;
            a_cursor
                .cmp(&b_cursor)
                .then_with(|| {
                    a.sealed
                        .metadata
                        .device_id
                        .cmp(&b.sealed.metadata.device_id)
                })
                .then_with(|| a.key.cmp(&b.key))
        });

        let mut total_merged = 0;
        let mut all_conflicts = Vec::new();
        let mut deltas_applied = 0;
        let mut max_remote_cursor = manifest.last_pull_cursor;

        for item in pending {
            let bundle = open_delta(&item.sealed, ns_key)?;

            // Merge records
            for remote_record in &bundle.records {
                let existing = self.storage.get_record(&remote_record.record_id, branch)?;
                match existing {
                    None => {
                        // New record — just insert
                        self.storage.put_record(remote_record.clone(), branch)?;
                        total_merged += 1;
                    }
                    Some(local_record) => {
                        // Record exists locally
                        if local_record.record_id == remote_record.record_id
                            && local_record.created_at_ms == remote_record.created_at_ms
                            && local_record.data == remote_record.data
                        {
                            // Identical — skip (idempotent)
                            continue;
                        }
                        // Conflict: different content for same logical record
                        let winner = resolve_conflict(strategy, &local_record, remote_record);
                        let conflict = SyncConflict {
                            record_id: remote_record.record_id.clone(),
                            local_record,
                            remote_record: remote_record.clone(),
                            resolved: Some(winner.clone()),
                        };
                        // If winner is different from local, update
                        if winner.record_id != conflict.local_record.record_id
                            || winner.data != conflict.local_record.data
                        {
                            self.storage.put_record(winner, branch)?;
                            total_merged += 1;
                        }
                        all_conflicts.push(conflict);
                    }
                }
            }

            // Apply tombstones after record upserts.
            for tombstone_id in &bundle.tombstones {
                if self.storage.get_record(tombstone_id, branch)?.is_some() {
                    total_merged += 1;
                }
                self.storage.delete_record(tombstone_id, branch)?;
            }

            // Track highest remote cursor
            if bundle.to_cursor > max_remote_cursor {
                max_remote_cursor = bundle.to_cursor;
            }
            deltas_applied += 1;
        }

        // Update manifest
        manifest.last_pull_cursor = max_remote_cursor;
        manifest.updated_at_ms = now_ms;
        self.save_manifest(&manifest, backend)?;

        Ok(PullResult {
            records_merged: total_merged,
            conflicts: all_conflicts,
            deltas_applied,
        })
    }

    /// Get the current sync status for a namespace.
    pub fn status(
        &self,
        namespace: &str,
        branch: &str,
        backend: &dyn SyncBackend,
    ) -> Result<SyncStatus, SyncError> {
        let manifest = self.load_or_create_manifest_readonly(namespace, backend)?;

        // Count records newer than last push
        let all_records =
            self.storage
                .list_records_by_namespace(namespace, branch, usize::MAX, 0)?;

        let pending = all_records
            .iter()
            .filter(|r| r.created_at_ms > manifest.last_push_cursor)
            .count() as u64;

        Ok(SyncStatus {
            namespace: namespace.into(),
            last_push: if manifest.last_push_cursor > 0 {
                Some(manifest.last_push_cursor)
            } else {
                None
            },
            last_pull: if manifest.last_pull_cursor > 0 {
                Some(manifest.last_pull_cursor)
            } else {
                None
            },
            pending_push_count: pending,
        })
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    /// Load manifest from backend, or create a fresh one.
    fn load_or_create_manifest(
        &self,
        namespace: &str,
        backend: &dyn SyncBackend,
    ) -> Result<SyncManifest, SyncError> {
        let key = format!("{}/manifests/{}.json", namespace, self.device_id);
        match backend.get_blob(&key)? {
            Some(bytes) => Ok(serde_json::from_slice(&bytes)?),
            None => Ok(SyncManifest::new(&self.device_id, namespace)),
        }
    }

    /// Same as load_or_create_manifest but doesn't need &mut backend.
    fn load_or_create_manifest_readonly(
        &self,
        namespace: &str,
        backend: &dyn SyncBackend,
    ) -> Result<SyncManifest, SyncError> {
        self.load_or_create_manifest(namespace, backend)
    }

    /// Save manifest to backend.
    fn save_manifest(
        &self,
        manifest: &SyncManifest,
        backend: &mut dyn SyncBackend,
    ) -> Result<(), SyncError> {
        let key = format!("{}/manifests/{}.json", manifest.namespace, self.device_id);
        let bytes = serde_json::to_vec(manifest)?;
        backend.put_blob(&key, &bytes)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::delta::seal_delta;
    use crate::memory_backend::MemorySyncBackend;
    use hellodb_core::Namespace;
    use hellodb_crypto::{KeyPair, MasterKey};
    use hellodb_storage::MemoryEngine;
    use serde_json::json;

    /// Helper: set up a storage engine with a namespace and some records.
    fn setup_device(_device_id: &str, owner: &KeyPair) -> MemoryEngine {
        let mut engine = MemoryEngine::new();
        let ns = Namespace::new(
            "commerce".into(),
            "Commerce".into(),
            owner.verifying.clone(),
            false,
        );
        engine.create_namespace(ns).unwrap();
        engine
    }

    fn write_record(
        engine: &mut MemoryEngine,
        kp: &KeyPair,
        data: serde_json::Value,
        ts: u64,
    ) -> Record {
        let rec = Record::new_with_timestamp(
            &kp.signing,
            "commerce.listing".into(),
            "commerce".into(),
            data,
            None,
            ts,
        )
        .unwrap();
        engine.put_record(rec.clone(), "commerce/main").unwrap();
        rec
    }

    #[test]
    fn push_uploads_delta() {
        let owner = KeyPair::generate();
        let mk = MasterKey::generate();
        let ns_key = mk.derive_namespace_key("commerce");
        let mut engine = setup_device("device-a", &owner);
        let mut backend = MemorySyncBackend::new();

        write_record(
            &mut engine,
            &owner,
            json!({"title": "Bowl", "price": 24.99}),
            1000,
        );
        write_record(
            &mut engine,
            &owner,
            json!({"title": "Vase", "price": 55.0}),
            2000,
        );

        let mut sync = SyncEngine::new(&mut engine, "device-a");
        let result = sync
            .push("commerce", "commerce/main", &ns_key, &mut backend, 3000)
            .unwrap();

        assert_eq!(result.records_pushed, 2);
        assert!(!result.delta_key.is_empty());
        assert!(backend.blob_count() >= 2); // delta + manifest
    }

    #[test]
    fn push_empty_is_noop() {
        let owner = KeyPair::generate();
        let mk = MasterKey::generate();
        let ns_key = mk.derive_namespace_key("commerce");
        let mut engine = setup_device("device-a", &owner);
        let mut backend = MemorySyncBackend::new();

        let mut sync = SyncEngine::new(&mut engine, "device-a");
        let result = sync
            .push("commerce", "commerce/main", &ns_key, &mut backend, 1000)
            .unwrap();

        assert_eq!(result.records_pushed, 0);
        assert!(result.delta_key.is_empty());
    }

    #[test]
    fn push_pull_roundtrip() {
        let owner = KeyPair::generate();
        let mk = MasterKey::generate();
        let ns_key = mk.derive_namespace_key("commerce");
        let mut backend = MemorySyncBackend::new();

        // Device A writes and pushes
        let mut engine_a = setup_device("device-a", &owner);
        write_record(
            &mut engine_a,
            &owner,
            json!({"title": "Bowl", "price": 24.99}),
            1000,
        );
        write_record(
            &mut engine_a,
            &owner,
            json!({"title": "Vase", "price": 55.0}),
            2000,
        );

        {
            let mut sync_a = SyncEngine::new(&mut engine_a, "device-a");
            sync_a
                .push("commerce", "commerce/main", &ns_key, &mut backend, 3000)
                .unwrap();
        }

        // Device B pulls
        let mut engine_b = setup_device("device-b", &owner);
        {
            let mut sync_b = SyncEngine::new(&mut engine_b, "device-b");
            let pull_result = sync_b
                .pull(
                    "commerce",
                    "commerce/main",
                    &ns_key,
                    &mut backend,
                    ConflictStrategy::LastWriterWins,
                    4000,
                )
                .unwrap();

            assert_eq!(pull_result.records_merged, 2);
            assert_eq!(pull_result.deltas_applied, 1);
            assert!(pull_result.conflicts.is_empty());
        }

        // Verify Device B has the records
        let records = engine_b
            .list_records_by_namespace("commerce", "commerce/main", 100, 0)
            .unwrap();
        assert_eq!(records.len(), 2);
    }

    #[test]
    fn multi_device_sync() {
        let owner = KeyPair::generate();
        let mk = MasterKey::generate();
        let ns_key = mk.derive_namespace_key("commerce");
        let mut backend = MemorySyncBackend::new();

        // Device A writes and pushes
        let mut engine_a = setup_device("device-a", &owner);
        write_record(&mut engine_a, &owner, json!({"title": "Bowl"}), 1000);

        {
            let mut sync_a = SyncEngine::new(&mut engine_a, "device-a");
            sync_a
                .push("commerce", "commerce/main", &ns_key, &mut backend, 2000)
                .unwrap();
        }

        // Device B writes, pushes, then pulls from A
        let mut engine_b = setup_device("device-b", &owner);
        write_record(&mut engine_b, &owner, json!({"title": "Vase"}), 1500);

        {
            let mut sync_b = SyncEngine::new(&mut engine_b, "device-b");
            sync_b
                .push("commerce", "commerce/main", &ns_key, &mut backend, 2500)
                .unwrap();
            let pull = sync_b
                .pull(
                    "commerce",
                    "commerce/main",
                    &ns_key,
                    &mut backend,
                    ConflictStrategy::LastWriterWins,
                    3000,
                )
                .unwrap();
            assert_eq!(pull.records_merged, 1); // Bowl from A
        }

        // Device A pulls from B
        {
            let mut sync_a = SyncEngine::new(&mut engine_a, "device-a");
            let pull = sync_a
                .pull(
                    "commerce",
                    "commerce/main",
                    &ns_key,
                    &mut backend,
                    ConflictStrategy::LastWriterWins,
                    3500,
                )
                .unwrap();
            assert_eq!(pull.records_merged, 1); // Vase from B
        }

        // Both should have 2 records
        let a_records = engine_a
            .list_records_by_namespace("commerce", "commerce/main", 100, 0)
            .unwrap();
        let b_records = engine_b
            .list_records_by_namespace("commerce", "commerce/main", 100, 0)
            .unwrap();
        assert_eq!(a_records.len(), 2);
        assert_eq!(b_records.len(), 2);
    }

    #[test]
    fn conflict_detection_lww() {
        let owner_a = KeyPair::generate();
        let owner_b = KeyPair::generate();
        let mk = MasterKey::generate();
        let ns_key = mk.derive_namespace_key("commerce");
        let mut backend = MemorySyncBackend::new();

        // Device A writes a record
        let mut engine_a = setup_device("device-a", &owner_a);
        write_record(
            &mut engine_a,
            &owner_a,
            json!({"title": "Bowl", "price": 20.0}),
            1000,
        );

        {
            let mut sync_a = SyncEngine::new(&mut engine_a, "device-a");
            sync_a
                .push("commerce", "commerce/main", &ns_key, &mut backend, 2000)
                .unwrap();
        }

        // Device B pulls, then writes a DIFFERENT record with same schema
        let mut engine_b = setup_device("device-b", &owner_b);
        {
            let mut sync_b = SyncEngine::new(&mut engine_b, "device-b");
            sync_b
                .pull(
                    "commerce",
                    "commerce/main",
                    &ns_key,
                    &mut backend,
                    ConflictStrategy::LastWriterWins,
                    2500,
                )
                .unwrap();
        }

        // Device B modifies the record (creates new version with same content key path)
        write_record(
            &mut engine_b,
            &owner_b,
            json!({"title": "Bowl Deluxe", "price": 30.0}),
            3000,
        );

        // Device A also writes new data
        write_record(
            &mut engine_a,
            &owner_a,
            json!({"title": "Candle", "price": 15.0}),
            3500,
        );

        // Both push
        {
            let mut sync_b = SyncEngine::new(&mut engine_b, "device-b");
            sync_b
                .push("commerce", "commerce/main", &ns_key, &mut backend, 4000)
                .unwrap();
        }
        {
            let mut sync_a = SyncEngine::new(&mut engine_a, "device-a");
            sync_a
                .push("commerce", "commerce/main", &ns_key, &mut backend, 4500)
                .unwrap();
        }

        // Device A pulls — should get B's records (new ones, no true conflict
        // since these are different record_ids in a content-addressed system)
        {
            let mut sync_a = SyncEngine::new(&mut engine_a, "device-a");
            let pull = sync_a
                .pull(
                    "commerce",
                    "commerce/main",
                    &ns_key,
                    &mut backend,
                    ConflictStrategy::LastWriterWins,
                    5000,
                )
                .unwrap();
            // New records from B that A doesn't have
            assert!(pull.records_merged > 0);
        }
    }

    #[test]
    fn idempotent_pull() {
        let owner = KeyPair::generate();
        let mk = MasterKey::generate();
        let ns_key = mk.derive_namespace_key("commerce");
        let mut backend = MemorySyncBackend::new();

        // Device A pushes
        let mut engine_a = setup_device("device-a", &owner);
        write_record(&mut engine_a, &owner, json!({"title": "Bowl"}), 1000);
        {
            let mut sync_a = SyncEngine::new(&mut engine_a, "device-a");
            sync_a
                .push("commerce", "commerce/main", &ns_key, &mut backend, 2000)
                .unwrap();
        }

        // Device B pulls twice — second pull should be a no-op
        let mut engine_b = setup_device("device-b", &owner);
        {
            let mut sync_b = SyncEngine::new(&mut engine_b, "device-b");
            let pull1 = sync_b
                .pull(
                    "commerce",
                    "commerce/main",
                    &ns_key,
                    &mut backend,
                    ConflictStrategy::LastWriterWins,
                    3000,
                )
                .unwrap();
            assert_eq!(pull1.records_merged, 1);
        }

        // Second pull — no new deltas because pull cursor is persisted.
        {
            let mut sync_b = SyncEngine::new(&mut engine_b, "device-b");
            let pull2 = sync_b
                .pull(
                    "commerce",
                    "commerce/main",
                    &ns_key,
                    &mut backend,
                    ConflictStrategy::LastWriterWins,
                    4000,
                )
                .unwrap();
            assert_eq!(pull2.records_merged, 0);
            assert_eq!(pull2.deltas_applied, 0);
            assert_eq!(pull2.conflicts.len(), 0);
        }
    }

    #[test]
    fn sync_status() {
        let owner = KeyPair::generate();
        let mk = MasterKey::generate();
        let ns_key = mk.derive_namespace_key("commerce");
        let mut engine = setup_device("device-a", &owner);
        let mut backend = MemorySyncBackend::new();

        write_record(&mut engine, &owner, json!({"title": "Bowl"}), 1000);
        write_record(&mut engine, &owner, json!({"title": "Vase"}), 2000);

        // Before push: 2 pending
        {
            let sync = SyncEngine::new(&mut engine, "device-a");
            let status = sync.status("commerce", "commerce/main", &backend).unwrap();
            assert_eq!(status.pending_push_count, 2);
            assert!(status.last_push.is_none());
        }

        // After push: 0 pending
        {
            let mut sync = SyncEngine::new(&mut engine, "device-a");
            sync.push("commerce", "commerce/main", &ns_key, &mut backend, 3000)
                .unwrap();
        }
        {
            let sync = SyncEngine::new(&mut engine, "device-a");
            let status = sync.status("commerce", "commerce/main", &backend).unwrap();
            assert_eq!(status.pending_push_count, 0);
            assert_eq!(status.last_push, Some(2000));
        }
    }

    #[test]
    fn incremental_push() {
        let owner = KeyPair::generate();
        let mk = MasterKey::generate();
        let ns_key = mk.derive_namespace_key("commerce");
        let mut engine = setup_device("device-a", &owner);
        let mut backend = MemorySyncBackend::new();

        // First batch
        write_record(&mut engine, &owner, json!({"title": "Bowl"}), 1000);
        {
            let mut sync = SyncEngine::new(&mut engine, "device-a");
            let r = sync
                .push("commerce", "commerce/main", &ns_key, &mut backend, 2000)
                .unwrap();
            assert_eq!(r.records_pushed, 1);
        }

        // Second batch — only new records
        write_record(&mut engine, &owner, json!({"title": "Vase"}), 3000);
        write_record(&mut engine, &owner, json!({"title": "Candle"}), 4000);
        {
            let mut sync = SyncEngine::new(&mut engine, "device-a");
            let r = sync
                .push("commerce", "commerce/main", &ns_key, &mut backend, 5000)
                .unwrap();
            assert_eq!(r.records_pushed, 2); // Only new records
        }
    }

    #[test]
    fn push_includes_new_tombstones_once() {
        let owner = KeyPair::generate();
        let mk = MasterKey::generate();
        let ns_key = mk.derive_namespace_key("commerce");
        let mut engine = setup_device("device-a", &owner);
        let mut backend = MemorySyncBackend::new();

        let rec = write_record(&mut engine, &owner, json!({"title": "Bowl"}), 1000);
        {
            let mut sync = SyncEngine::new(&mut engine, "device-a");
            let first = sync
                .push("commerce", "commerce/main", &ns_key, &mut backend, 2000)
                .unwrap();
            assert_eq!(first.records_pushed, 1);
            assert_eq!(first.tombstones_pushed, 0);
        }

        engine
            .delete_record(&rec.record_id, "commerce/main")
            .unwrap();
        {
            let mut sync = SyncEngine::new(&mut engine, "device-a");
            let second = sync
                .push("commerce", "commerce/main", &ns_key, &mut backend, 3000)
                .unwrap();
            assert_eq!(second.records_pushed, 0);
            assert_eq!(second.tombstones_pushed, 1);
        }

        // Tombstone was already published; third push is a no-op.
        {
            let mut sync = SyncEngine::new(&mut engine, "device-a");
            let third = sync
                .push("commerce", "commerce/main", &ns_key, &mut backend, 4000)
                .unwrap();
            assert_eq!(third.records_pushed, 0);
            assert_eq!(third.tombstones_pushed, 0);
            assert!(third.delta_key.is_empty());
        }
    }

    #[test]
    fn pull_applies_remote_tombstones() {
        let owner = KeyPair::generate();
        let mk = MasterKey::generate();
        let ns_key = mk.derive_namespace_key("commerce");
        let mut backend = MemorySyncBackend::new();

        let mut engine_a = setup_device("device-a", &owner);
        let rec = write_record(&mut engine_a, &owner, json!({"title": "Bowl"}), 1000);
        {
            let mut sync_a = SyncEngine::new(&mut engine_a, "device-a");
            sync_a
                .push("commerce", "commerce/main", &ns_key, &mut backend, 1500)
                .unwrap();
        }

        let mut engine_b = setup_device("device-b", &owner);
        {
            let mut sync_b = SyncEngine::new(&mut engine_b, "device-b");
            sync_b
                .pull(
                    "commerce",
                    "commerce/main",
                    &ns_key,
                    &mut backend,
                    ConflictStrategy::LastWriterWins,
                    2000,
                )
                .unwrap();
        }
        assert!(engine_b
            .get_record(&rec.record_id, "commerce/main")
            .unwrap()
            .is_some());

        engine_a
            .delete_record(&rec.record_id, "commerce/main")
            .unwrap();
        {
            let mut sync_a = SyncEngine::new(&mut engine_a, "device-a");
            let pushed = sync_a
                .push("commerce", "commerce/main", &ns_key, &mut backend, 3000)
                .unwrap();
            assert_eq!(pushed.tombstones_pushed, 1);
        }

        {
            let mut sync_b = SyncEngine::new(&mut engine_b, "device-b");
            sync_b
                .pull(
                    "commerce",
                    "commerce/main",
                    &ns_key,
                    &mut backend,
                    ConflictStrategy::LastWriterWins,
                    4000,
                )
                .unwrap();
        }
        assert!(engine_b
            .get_record(&rec.record_id, "commerce/main")
            .unwrap()
            .is_none());
    }

    #[test]
    fn pull_orders_deltas_by_cursor_before_merge() {
        let owner = KeyPair::generate();
        let mk = MasterKey::generate();
        let ns_key = mk.derive_namespace_key("commerce");
        let mut backend = MemorySyncBackend::new();

        let rec = Record::new_with_timestamp(
            &owner.signing,
            "commerce.listing".into(),
            "commerce".into(),
            json!({"title": "Bowl"}),
            None,
            1000,
        )
        .unwrap();

        // Old delta: add record at cursor 1000.
        let add = DeltaBundle {
            device_id: "device-z".into(),
            namespace: "commerce".into(),
            branch: "commerce/main".into(),
            from_cursor: 0,
            to_cursor: 1000,
            records: vec![rec.clone()],
            tombstones: vec![],
            created_at_ms: 1001,
        };
        // New delta: tombstone same record at cursor 2000.
        let del = DeltaBundle {
            device_id: "device-a".into(),
            namespace: "commerce".into(),
            branch: "commerce/main".into(),
            from_cursor: 1000,
            to_cursor: 2000,
            records: vec![],
            tombstones: vec![rec.record_id.clone()],
            created_at_ms: 2001,
        };

        // Intentionally write keys so lexicographic backend listing returns
        // device-a/2000 BEFORE device-z/1000; cursor ordering must still win.
        let del_bytes = serde_json::to_vec(&seal_delta(&del, &ns_key).unwrap()).unwrap();
        backend
            .put_blob("commerce/deltas/device-a/2000.delta", &del_bytes)
            .unwrap();
        let add_bytes = serde_json::to_vec(&seal_delta(&add, &ns_key).unwrap()).unwrap();
        backend
            .put_blob("commerce/deltas/device-z/1000.delta", &add_bytes)
            .unwrap();

        let mut receiver = setup_device("device-b", &owner);
        let mut sync_b = SyncEngine::new(&mut receiver, "device-b");
        let pull = sync_b
            .pull(
                "commerce",
                "commerce/main",
                &ns_key,
                &mut backend,
                ConflictStrategy::LastWriterWins,
                3000,
            )
            .unwrap();
        assert_eq!(pull.deltas_applied, 2);
        assert!(receiver
            .get_record(&rec.record_id, "commerce/main")
            .unwrap()
            .is_none());
    }
}
