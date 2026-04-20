//! Phase 1 Integration Tests — Query Engine + Sync Layer end-to-end.
//!
//! These tests exercise the full stack: storage → auth → query + sync
//! working together as the "inverse Lakebase" on a single device and
//! across simulated multi-device sync.

use hellodb_auth::{AccessGate, DelegationCredential, DelegationScope};
use hellodb_core::{FieldType, Namespace, Record, Schema, SchemaField};
use hellodb_crypto::{KeyPair, MasterKey};
use hellodb_query::{Filter, Query, QueryEngine, SortField};
use hellodb_storage::{MemoryEngine, StorageEngine};
use hellodb_sync::{ConflictStrategy, MemorySyncBackend, SyncEngine};
use serde_json::json;

// -------------------------------------------------------------------------
// Query Engine Integration
// -------------------------------------------------------------------------

/// End-to-end: create namespace + schema, write records, query with filter + sort + pagination.
#[test]
fn query_filter_sort_paginate() {
    let mut engine = MemoryEngine::new();
    let owner = KeyPair::generate();

    // Create namespace + schema
    let ns = Namespace::new(
        "commerce".into(),
        "Commerce".into(),
        owner.verifying.clone(),
        false,
    );
    engine.create_namespace(ns).unwrap();

    let schema = Schema {
        id: "commerce.listing".into(),
        version: "1".into(),
        namespace: "commerce".into(),
        name: "Listing".into(),
        fields: vec![
            SchemaField {
                name: "title".into(),
                field_type: FieldType::String,
                required: true,
                description: None,
            },
            SchemaField {
                name: "price".into(),
                field_type: FieldType::Float,
                required: true,
                description: None,
            },
            SchemaField {
                name: "currency".into(),
                field_type: FieldType::String,
                required: true,
                description: None,
            },
        ],
        registered_at_ms: 1000,
    };
    engine.register_schema(schema).unwrap();

    // Write test records with known prices
    let listings = vec![
        ("Ceramic Bowl", 24.99, "USD", 1000u64),
        ("Honey Jar", 12.50, "USD", 1001),
        ("Woven Basket", 39.99, "EUR", 1002),
        ("Glass Vase", 55.00, "USD", 1003),
        ("Candle Set", 18.75, "EUR", 1004),
        ("Silk Scarf", 42.00, "USD", 1005),
        ("Tea Set", 85.00, "EUR", 1006),
    ];

    for (title, price, currency, ts) in &listings {
        let rec = Record::new_with_timestamp(
            &owner.signing,
            "commerce.listing".into(),
            "commerce".into(),
            json!({"title": title, "price": price, "currency": currency}),
            None,
            *ts,
        )
        .unwrap();
        engine.put_record(rec, "commerce/main").unwrap();
    }

    let gate = AccessGate::new();
    let qe = QueryEngine::new(&engine, &gate);

    // 1. Filter by currency
    let usd_only = qe
        .execute(
            &Query::new()
                .namespace("commerce")
                .filter(Filter::Eq("currency".into(), json!("USD"))),
            &owner.verifying,
            "commerce/main",
            5000,
        )
        .unwrap();
    assert_eq!(usd_only.records.len(), 4); // Bowl, Honey, Vase, Scarf

    // 2. Filter + sort by price ascending
    let sorted = qe
        .execute(
            &Query::new()
                .namespace("commerce")
                .filter(Filter::Eq("currency".into(), json!("USD")))
                .sort(SortField::asc("price")),
            &owner.verifying,
            "commerce/main",
            5000,
        )
        .unwrap();
    let prices: Vec<f64> = sorted
        .records
        .iter()
        .map(|r| r.data["price"].as_f64().unwrap())
        .collect();
    assert_eq!(prices, vec![12.5, 24.99, 42.0, 55.0]);

    // 3. Paginated: page 1 of 2
    let page1 = qe
        .execute(
            &Query::new()
                .namespace("commerce")
                .sort(SortField::asc("price"))
                .limit(3),
            &owner.verifying,
            "commerce/main",
            5000,
        )
        .unwrap();
    assert_eq!(page1.records.len(), 3);
    assert!(page1.has_more);
    assert!(page1.next_cursor.is_some());

    // Page 2 via cursor
    let page2 = qe
        .execute(
            &Query::new()
                .namespace("commerce")
                .sort(SortField::asc("price"))
                .limit(3)
                .after(page1.next_cursor.unwrap()),
            &owner.verifying,
            "commerce/main",
            5000,
        )
        .unwrap();
    assert_eq!(page2.records.len(), 3);
    // No overlap between pages
    for p1_rec in &page1.records {
        for p2_rec in &page2.records {
            assert_ne!(p1_rec.record_id, p2_rec.record_id);
        }
    }

    // 4. Range filter: 20 <= price <= 50
    let range = qe
        .execute(
            &Query::new().namespace("commerce").filter(Filter::And(vec![
                Filter::Gte("price".into(), json!(20.0)),
                Filter::Lte("price".into(), json!(50.0)),
            ])),
            &owner.verifying,
            "commerce/main",
            5000,
        )
        .unwrap();
    // 24.99, 39.99, 42.00
    assert_eq!(range.records.len(), 3);

    // 5. Count query
    let count = qe
        .count(
            &Query::new()
                .namespace("commerce")
                .schema("commerce.listing"),
            &owner.verifying,
            "commerce/main",
            5000,
        )
        .unwrap();
    assert_eq!(count, 7);
}

/// Cross-namespace agent query with delegation.
#[test]
fn cross_namespace_agent_query() {
    let mut engine = MemoryEngine::new();
    let owner = KeyPair::generate();
    let agent = KeyPair::generate();

    // Create two namespaces
    for (ns_id, ns_name) in &[("commerce", "Commerce"), ("health", "Health")] {
        let ns = Namespace::new(
            ns_id.to_string(),
            ns_name.to_string(),
            owner.verifying.clone(),
            false,
        );
        engine.create_namespace(ns).unwrap();
    }

    // Write records in both
    let rec_commerce = Record::new_with_timestamp(
        &owner.signing,
        "commerce.listing".into(),
        "commerce".into(),
        json!({"title": "Bowl", "price": 25.0}),
        None,
        1000,
    )
    .unwrap();
    engine.put_record(rec_commerce, "commerce/main").unwrap();

    let rec_health = Record::new_with_timestamp(
        &owner.signing,
        "health.vitals".into(),
        "health".into(),
        json!({"bpm": 72, "device": "watch"}),
        None,
        2000,
    )
    .unwrap();
    engine.put_record(rec_health, "health/main").unwrap();

    // Without delegation: denied
    let gate_no_deleg = AccessGate::new();
    let qe_denied = QueryEngine::new(&engine, &gate_no_deleg);
    let result = qe_denied.execute_cross_namespace(
        &Query::new(),
        &agent.verifying,
        &[("commerce", "commerce/main"), ("health", "health/main")],
        5000,
    );
    assert!(result.is_err());

    // With delegation: succeeds
    let deleg = DelegationCredential::new(
        &owner.signing,
        agent.verifying.clone(),
        vec![
            DelegationScope::CrossNamespaceQuery,
            DelegationScope::ReadNamespace,
        ],
        vec!["commerce".into(), "health".into()],
        1000,
        3_600_000,
        100,
    )
    .unwrap();

    let mut gate = AccessGate::new();
    gate.add_delegation(deleg).unwrap();

    let qe = QueryEngine::new(&engine, &gate);
    let result = qe
        .execute_cross_namespace(
            &Query::new(),
            &agent.verifying,
            &[("commerce", "commerce/main"), ("health", "health/main")],
            5000,
        )
        .unwrap();

    assert_eq!(result.records.len(), 2); // 1 commerce + 1 health
}

// -------------------------------------------------------------------------
// Sync Layer Integration
// -------------------------------------------------------------------------

/// End-to-end: Device A writes → pushes → Device B pulls → converges.
#[test]
fn sync_push_pull_converge() {
    let owner = KeyPair::generate();
    let mk = MasterKey::generate();
    let ns_key = mk.derive_namespace_key("commerce");
    let mut backend = MemorySyncBackend::new();

    // Device A: create namespace, write records, push
    let mut engine_a = MemoryEngine::new();
    engine_a
        .create_namespace(Namespace::new(
            "commerce".into(),
            "Commerce".into(),
            owner.verifying.clone(),
            false,
        ))
        .unwrap();

    for (title, price, ts) in &[("Bowl", 24.99, 1000u64), ("Vase", 55.0, 2000)] {
        let rec = Record::new_with_timestamp(
            &owner.signing,
            "commerce.listing".into(),
            "commerce".into(),
            json!({"title": title, "price": price}),
            None,
            *ts,
        )
        .unwrap();
        engine_a.put_record(rec, "commerce/main").unwrap();
    }

    {
        let mut sync_a = SyncEngine::new(&mut engine_a, "device-a");
        let push_result = sync_a
            .push("commerce", "commerce/main", &ns_key, &mut backend, 3000)
            .unwrap();
        assert_eq!(push_result.records_pushed, 2);
    }

    // Device B: create same namespace, pull
    let mut engine_b = MemoryEngine::new();
    engine_b
        .create_namespace(Namespace::new(
            "commerce".into(),
            "Commerce".into(),
            owner.verifying.clone(),
            false,
        ))
        .unwrap();

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

    // Verify convergence: both devices have the same records
    let recs_a = engine_a
        .list_records_by_namespace("commerce", "commerce/main", 100, 0)
        .unwrap();
    let recs_b = engine_b
        .list_records_by_namespace("commerce", "commerce/main", 100, 0)
        .unwrap();
    assert_eq!(recs_a.len(), 2);
    assert_eq!(recs_b.len(), 2);

    // Same record IDs
    let ids_a: Vec<&str> = recs_a.iter().map(|r| r.record_id.as_str()).collect();
    for rec_b in &recs_b {
        assert!(ids_a.contains(&rec_b.record_id.as_str()));
    }
}

/// Multi-device bidirectional sync: A pushes, B pushes, both pull to converge.
#[test]
fn sync_bidirectional_convergence() {
    let owner = KeyPair::generate();
    let mk = MasterKey::generate();
    let ns_key = mk.derive_namespace_key("commerce");
    let mut backend = MemorySyncBackend::new();

    let make_engine = |owner: &KeyPair| -> MemoryEngine {
        let mut e = MemoryEngine::new();
        e.create_namespace(Namespace::new(
            "commerce".into(),
            "Commerce".into(),
            owner.verifying.clone(),
            false,
        ))
        .unwrap();
        e
    };

    let write = |engine: &mut MemoryEngine, owner: &KeyPair, title: &str, ts: u64| {
        let rec = Record::new_with_timestamp(
            &owner.signing,
            "commerce.listing".into(),
            "commerce".into(),
            json!({"title": title}),
            None,
            ts,
        )
        .unwrap();
        engine.put_record(rec, "commerce/main").unwrap();
    };

    // Device A writes Bowl
    let mut engine_a = make_engine(&owner);
    write(&mut engine_a, &owner, "Bowl", 1000);

    // Device B writes Vase
    let mut engine_b = make_engine(&owner);
    write(&mut engine_b, &owner, "Vase", 1500);

    // Both push
    {
        let mut sync_a = SyncEngine::new(&mut engine_a, "device-a");
        sync_a
            .push("commerce", "commerce/main", &ns_key, &mut backend, 2000)
            .unwrap();
    }
    {
        let mut sync_b = SyncEngine::new(&mut engine_b, "device-b");
        sync_b
            .push("commerce", "commerce/main", &ns_key, &mut backend, 2500)
            .unwrap();
    }

    // Both pull
    {
        let mut sync_a = SyncEngine::new(&mut engine_a, "device-a");
        let pull = sync_a
            .pull(
                "commerce",
                "commerce/main",
                &ns_key,
                &mut backend,
                ConflictStrategy::LastWriterWins,
                3000,
            )
            .unwrap();
        assert_eq!(pull.records_merged, 1); // Vase from B
    }
    {
        let mut sync_b = SyncEngine::new(&mut engine_b, "device-b");
        let pull = sync_b
            .pull(
                "commerce",
                "commerce/main",
                &ns_key,
                &mut backend,
                ConflictStrategy::LastWriterWins,
                3500,
            )
            .unwrap();
        assert_eq!(pull.records_merged, 1); // Bowl from A
    }

    // Both should have 2 records
    let recs_a = engine_a
        .list_records_by_namespace("commerce", "commerce/main", 100, 0)
        .unwrap();
    let recs_b = engine_b
        .list_records_by_namespace("commerce", "commerce/main", 100, 0)
        .unwrap();
    assert_eq!(recs_a.len(), 2);
    assert_eq!(recs_b.len(), 2);
}

/// Encryption isolation: wrong namespace key can't read synced data.
#[test]
fn sync_encryption_isolation() {
    let owner = KeyPair::generate();
    let mk = MasterKey::generate();
    let ns_key = mk.derive_namespace_key("commerce");
    let wrong_key = mk.derive_namespace_key("health"); // Different namespace key
    let mut backend = MemorySyncBackend::new();

    // Device A pushes encrypted data
    let mut engine_a = MemoryEngine::new();
    engine_a
        .create_namespace(Namespace::new(
            "commerce".into(),
            "Commerce".into(),
            owner.verifying.clone(),
            false,
        ))
        .unwrap();

    let rec = Record::new_with_timestamp(
        &owner.signing,
        "commerce.listing".into(),
        "commerce".into(),
        json!({"title": "Secret Bowl", "price": 999.99}),
        None,
        1000,
    )
    .unwrap();
    engine_a.put_record(rec, "commerce/main").unwrap();

    {
        let mut sync_a = SyncEngine::new(&mut engine_a, "device-a");
        sync_a
            .push("commerce", "commerce/main", &ns_key, &mut backend, 2000)
            .unwrap();
    }

    // Device B tries to pull with WRONG key — should fail to decrypt
    let mut engine_b = MemoryEngine::new();
    engine_b
        .create_namespace(Namespace::new(
            "commerce".into(),
            "Commerce".into(),
            owner.verifying.clone(),
            false,
        ))
        .unwrap();

    let mut sync_b = SyncEngine::new(&mut engine_b, "device-b");
    let result = sync_b.pull(
        "commerce",
        "commerce/main",
        &wrong_key,
        &mut backend,
        ConflictStrategy::LastWriterWins,
        3000,
    );

    // Should fail with decryption error
    assert!(result.is_err());
}

// -------------------------------------------------------------------------
// Query + Sync Combined
// -------------------------------------------------------------------------

/// Full pipeline: write → push → pull → query on receiving device.
#[test]
fn query_after_sync() {
    let owner = KeyPair::generate();
    let mk = MasterKey::generate();
    let ns_key = mk.derive_namespace_key("commerce");
    let mut backend = MemorySyncBackend::new();

    // Device A: write diverse records and push
    let mut engine_a = MemoryEngine::new();
    engine_a
        .create_namespace(Namespace::new(
            "commerce".into(),
            "Commerce".into(),
            owner.verifying.clone(),
            false,
        ))
        .unwrap();

    let items = vec![
        ("Ceramic Bowl", 24.99, "USD", 1000u64),
        ("Honey Jar", 12.50, "USD", 1001),
        ("Woven Basket", 39.99, "EUR", 1002),
        ("Glass Vase", 55.00, "USD", 1003),
    ];

    for (title, price, currency, ts) in &items {
        let rec = Record::new_with_timestamp(
            &owner.signing,
            "commerce.listing".into(),
            "commerce".into(),
            json!({"title": title, "price": price, "currency": currency}),
            None,
            *ts,
        )
        .unwrap();
        engine_a.put_record(rec, "commerce/main").unwrap();
    }

    {
        let mut sync_a = SyncEngine::new(&mut engine_a, "device-a");
        sync_a
            .push("commerce", "commerce/main", &ns_key, &mut backend, 5000)
            .unwrap();
    }

    // Device B: create namespace, pull, then query
    let mut engine_b = MemoryEngine::new();
    engine_b
        .create_namespace(Namespace::new(
            "commerce".into(),
            "Commerce".into(),
            owner.verifying.clone(),
            false,
        ))
        .unwrap();

    {
        let mut sync_b = SyncEngine::new(&mut engine_b, "device-b");
        sync_b
            .pull(
                "commerce",
                "commerce/main",
                &ns_key,
                &mut backend,
                ConflictStrategy::LastWriterWins,
                6000,
            )
            .unwrap();
    }

    // Now query on Device B — should see all records from A
    let gate = AccessGate::new();
    let qe = QueryEngine::new(&engine_b, &gate);

    // All records
    let all = qe
        .execute(
            &Query::new().namespace("commerce"),
            &owner.verifying,
            "commerce/main",
            7000,
        )
        .unwrap();
    assert_eq!(all.records.len(), 4);

    // Filtered: USD only, sorted by price desc
    let usd_sorted = qe
        .execute(
            &Query::new()
                .namespace("commerce")
                .filter(Filter::Eq("currency".into(), json!("USD")))
                .sort(SortField::desc("price")),
            &owner.verifying,
            "commerce/main",
            7000,
        )
        .unwrap();
    assert_eq!(usd_sorted.records.len(), 3);
    let prices: Vec<f64> = usd_sorted
        .records
        .iter()
        .map(|r| r.data["price"].as_f64().unwrap())
        .collect();
    assert_eq!(prices, vec![55.0, 24.99, 12.5]);

    // Contains filter
    let ceramic = qe
        .execute(
            &Query::new()
                .namespace("commerce")
                .filter(Filter::Contains("title".into(), "Bowl".into())),
            &owner.verifying,
            "commerce/main",
            7000,
        )
        .unwrap();
    assert_eq!(ceramic.records.len(), 1);
    assert_eq!(ceramic.records[0].data["title"], "Ceramic Bowl");
}

/// Incremental sync: A pushes batch 1, B pulls, A pushes batch 2, B pulls again.
#[test]
fn incremental_sync_with_query() {
    let owner = KeyPair::generate();
    let mk = MasterKey::generate();
    let ns_key = mk.derive_namespace_key("commerce");
    let mut backend = MemorySyncBackend::new();

    let mut engine_a = MemoryEngine::new();
    engine_a
        .create_namespace(Namespace::new(
            "commerce".into(),
            "Commerce".into(),
            owner.verifying.clone(),
            false,
        ))
        .unwrap();

    // Batch 1: 2 records
    for (title, ts) in &[("Bowl", 1000u64), ("Vase", 2000)] {
        let rec = Record::new_with_timestamp(
            &owner.signing,
            "commerce.listing".into(),
            "commerce".into(),
            json!({"title": title, "price": 25.0}),
            None,
            *ts,
        )
        .unwrap();
        engine_a.put_record(rec, "commerce/main").unwrap();
    }

    {
        let mut sync_a = SyncEngine::new(&mut engine_a, "device-a");
        let r = sync_a
            .push("commerce", "commerce/main", &ns_key, &mut backend, 3000)
            .unwrap();
        assert_eq!(r.records_pushed, 2);
    }

    // Device B pulls batch 1
    let mut engine_b = MemoryEngine::new();
    engine_b
        .create_namespace(Namespace::new(
            "commerce".into(),
            "Commerce".into(),
            owner.verifying.clone(),
            false,
        ))
        .unwrap();

    {
        let mut sync_b = SyncEngine::new(&mut engine_b, "device-b");
        let pull = sync_b
            .pull(
                "commerce",
                "commerce/main",
                &ns_key,
                &mut backend,
                ConflictStrategy::LastWriterWins,
                4000,
            )
            .unwrap();
        assert_eq!(pull.records_merged, 2);
    }

    // Batch 2: A writes 2 more records
    for (title, ts) in &[("Candle", 5000u64), ("Scarf", 6000)] {
        let rec = Record::new_with_timestamp(
            &owner.signing,
            "commerce.listing".into(),
            "commerce".into(),
            json!({"title": title, "price": 30.0}),
            None,
            *ts,
        )
        .unwrap();
        engine_a.put_record(rec, "commerce/main").unwrap();
    }

    {
        let mut sync_a = SyncEngine::new(&mut engine_a, "device-a");
        let r = sync_a
            .push("commerce", "commerce/main", &ns_key, &mut backend, 7000)
            .unwrap();
        assert_eq!(r.records_pushed, 2); // Only new records
    }

    // Device B pulls batch 2
    {
        let mut sync_b = SyncEngine::new(&mut engine_b, "device-b");
        let pull = sync_b
            .pull(
                "commerce",
                "commerce/main",
                &ns_key,
                &mut backend,
                ConflictStrategy::LastWriterWins,
                8000,
            )
            .unwrap();
        assert_eq!(pull.records_merged, 2); // Only new records
    }

    // Verify Device B has all 4 records
    let gate = AccessGate::new();
    let qe = QueryEngine::new(&engine_b, &gate);
    let all = qe
        .execute(
            &Query::new().namespace("commerce"),
            &owner.verifying,
            "commerce/main",
            9000,
        )
        .unwrap();
    assert_eq!(all.records.len(), 4);
}
