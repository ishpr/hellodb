//! hellodb Local Storage Engine
//!
//! Provides trait-based storage with in-memory and SQLite/SQLCipher backends.
//! Also exposes an experimental write-ahead log module (not yet integrated
//! into the SqliteEngine mutation path).

pub mod engine;
pub mod error;
pub mod memory;
pub mod sqlite;
pub mod wal;

pub use engine::{decayed_score, RecordMetadata, StorageEngine, TailEntry};
pub use error::StorageError;
pub use memory::MemoryEngine;
pub use sqlite::SqliteEngine;

#[cfg(test)]
mod tests {
    //! Generic test suite that runs against both MemoryEngine and SqliteEngine.
    //! This ensures both backends behave identically.

    use super::*;
    use hellodb_core::*;
    use hellodb_crypto::KeyPair;
    use serde_json::json;

    fn make_namespace(kp: &KeyPair, id: &str, name: &str) -> Namespace {
        Namespace {
            id: id.into(),
            name: name.into(),
            owner: kp.verifying.clone(),
            description: None,
            created_at_ms: 1000,
            encrypted: true,
            schemas: Vec::new(),
        }
    }

    fn make_schema(ns: &str, id: &str) -> Schema {
        Schema {
            id: id.into(),
            version: "1.0.0".into(),
            namespace: ns.into(),
            name: "Test Schema".into(),
            fields: vec![SchemaField {
                name: "title".into(),
                field_type: FieldType::String,
                required: true,
                description: None,
            }],
            registered_at_ms: 2000,
        }
    }

    fn make_record(
        kp: &KeyPair,
        schema: &str,
        ns: &str,
        data: serde_json::Value,
        ts: u64,
    ) -> Record {
        Record::new_with_timestamp(&kp.signing, schema.into(), ns.into(), data, None, ts).unwrap()
    }

    /// Run the full test suite against a StorageEngine implementation.
    fn run_engine_tests(engine: &mut dyn StorageEngine) {
        let kp = KeyPair::generate();
        let ns_id = "test.ns";
        let main_branch = format!("{}/main", ns_id);
        let schema_id = "test.ns.items";

        // --- Namespace CRUD ---
        assert!(engine.list_namespaces().unwrap().is_empty());

        let ns = make_namespace(&kp, ns_id, "Test Namespace");
        engine.create_namespace(ns).unwrap();

        let fetched = engine.get_namespace(ns_id).unwrap().unwrap();
        assert_eq!(fetched.id, ns_id);
        assert_eq!(fetched.name, "Test Namespace");

        assert_eq!(engine.list_namespaces().unwrap().len(), 1);

        // Duplicate namespace should fail
        let ns2 = make_namespace(&kp, ns_id, "Duplicate");
        assert!(engine.create_namespace(ns2).is_err());

        // --- Schema registration ---
        let schema = make_schema(ns_id, schema_id);
        engine.register_schema(schema).unwrap();

        let fetched_schema = engine.get_schema(schema_id).unwrap().unwrap();
        assert_eq!(fetched_schema.id, schema_id);

        let schemas = engine.list_schemas(ns_id).unwrap();
        assert_eq!(schemas.len(), 1);

        // --- Branch creation ---
        let branches = engine.list_branches(ns_id).unwrap();
        assert_eq!(branches.len(), 1); // main was auto-created
        assert_eq!(branches[0].label, "main");

        let child = Branch::new(
            format!("{}/draft", ns_id),
            ns_id.into(),
            main_branch.clone(),
            "draft".into(),
        );
        engine.create_branch(child).unwrap();

        let branches = engine.list_branches(ns_id).unwrap();
        assert_eq!(branches.len(), 2);

        // --- Record put/get on main ---
        let rec1 = make_record(&kp, schema_id, ns_id, json!({"title": "Record 1"}), 3000);
        let rec1_id = rec1.record_id.clone();
        engine.put_record(rec1, &main_branch).unwrap();

        let fetched_rec = engine.get_record(&rec1_id, &main_branch).unwrap().unwrap();
        assert_eq!(fetched_rec.record_id, rec1_id);
        assert!(engine.has_record(&rec1_id, &main_branch).unwrap());

        // --- Record visible on child branch (inherited) ---
        let child_branch = format!("{}/draft", ns_id);
        assert!(engine.has_record(&rec1_id, &child_branch).unwrap());
        let inherited = engine.get_record(&rec1_id, &child_branch).unwrap().unwrap();
        assert_eq!(inherited.record_id, rec1_id);

        // --- Record on child branch only ---
        let rec2 = make_record(&kp, schema_id, ns_id, json!({"title": "Record 2"}), 4000);
        let rec2_id = rec2.record_id.clone();
        engine.put_record(rec2, &child_branch).unwrap();

        assert!(engine.has_record(&rec2_id, &child_branch).unwrap());
        assert!(!engine.has_record(&rec2_id, &main_branch).unwrap()); // not on main yet

        // --- List and count ---
        let main_records = engine
            .list_records_by_schema(schema_id, &main_branch, 100, 0)
            .unwrap();
        assert_eq!(main_records.len(), 1);

        let child_records = engine
            .list_records_by_schema(schema_id, &child_branch, 100, 0)
            .unwrap();
        assert_eq!(child_records.len(), 2); // rec1 (inherited) + rec2

        assert_eq!(
            engine
                .count_records_by_schema(schema_id, &main_branch)
                .unwrap(),
            1
        );
        assert_eq!(
            engine
                .count_records_by_schema(schema_id, &child_branch)
                .unwrap(),
            2
        );

        // --- List by namespace ---
        let ns_records = engine
            .list_records_by_namespace(ns_id, &child_branch, 100, 0)
            .unwrap();
        assert_eq!(ns_records.len(), 2);

        // --- Delete (tombstone) ---
        engine.delete_record(&rec1_id, &child_branch).unwrap();
        assert!(!engine.has_record(&rec1_id, &child_branch).unwrap()); // deleted on child
        assert!(engine.has_record(&rec1_id, &main_branch).unwrap()); // still on main

        let child_after_delete = engine
            .list_records_by_schema(schema_id, &child_branch, 100, 0)
            .unwrap();
        assert_eq!(child_after_delete.len(), 1); // only rec2

        // --- Merge ---
        // Create a fresh child for merge test
        let merge_child_id = format!("{}/merge-test", ns_id);
        let merge_child = Branch::new(
            merge_child_id.clone(),
            ns_id.into(),
            main_branch.clone(),
            "merge-test".into(),
        );
        engine.create_branch(merge_child).unwrap();

        let rec3 = make_record(&kp, schema_id, ns_id, json!({"title": "Record 3"}), 5000);
        let rec3_id = rec3.record_id.clone();
        engine.put_record(rec3, &merge_child_id).unwrap();

        // Before merge: rec3 not on main
        assert!(!engine.has_record(&rec3_id, &main_branch).unwrap());

        // Merge
        let result = engine.merge_branch(&merge_child_id).unwrap();
        assert_eq!(result.merged_records.len(), 1);
        assert!(result.conflicts.is_empty());

        // After merge: rec3 is on main
        assert!(engine.has_record(&rec3_id, &main_branch).unwrap());

        // Branch is now merged (not active)
        let merged_branch = engine.get_branch(&merge_child_id).unwrap().unwrap();
        assert_eq!(merged_branch.state, BranchState::Merged);

        // --- Deduplication ---
        let rec4 = make_record(&kp, schema_id, ns_id, json!({"title": "Record 4"}), 6000);
        let _rec4_id = rec4.record_id.clone();
        engine.put_record(rec4.clone(), &main_branch).unwrap();
        engine.put_record(rec4, &main_branch).unwrap(); // duplicate
                                                        // Should still only have one copy
        let count = engine
            .count_records_by_schema(schema_id, &main_branch)
            .unwrap();
        // rec1 + rec3 (merged) + rec4 = 3
        assert_eq!(count, 3);
    }

    #[test]
    fn memory_engine_full_suite() {
        let mut engine = MemoryEngine::new();
        run_engine_tests(&mut engine);
    }

    #[test]
    fn sqlite_engine_full_suite() {
        let mut engine = SqliteEngine::open_in_memory().unwrap();
        run_engine_tests(&mut engine);
    }
}
