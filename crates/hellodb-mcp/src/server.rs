//! MCP tool server for hellodb.
//!
//! Exposes a curated set of tools over stdio JSON-RPC so an MCP client
//! (Claude Code, Claude Desktop, Cursor, etc.) can use hellodb as a
//! sovereign, queryable, branchable memory store.

use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::{json, Value};

use hellodb_auth::AccessGate;
use hellodb_core::{Branch, FieldType, Namespace, Record, Schema, SchemaField};
use hellodb_crypto::KeyPair;
use hellodb_embed::build_from_env as build_embedder_from_env;
use hellodb_query::{Filter, Query, QueryEngine, SortField};
use hellodb_storage::{decayed_score, RecordMetadata, SqliteEngine, StorageEngine, TailEntry};
use hellodb_vector::VectorIndex;

use crate::protocol::{self, RpcError};

pub struct Server {
    storage: Mutex<SqliteEngine>,
    access: AccessGate,
    identity: KeyPair,
}

impl Server {
    pub fn new(storage: SqliteEngine, identity: KeyPair) -> Self {
        Self {
            storage: Mutex::new(storage),
            access: AccessGate::new(),
            identity,
        }
    }

    pub fn handle(&self, req: protocol::Request) -> Option<protocol::Response> {
        if req.is_notification() {
            return None;
        }
        let id = req.id.clone().unwrap_or(Value::Null);

        let result = match req.method.as_str() {
            "initialize" => Ok(self.initialize()),
            "tools/list" => Ok(self.list_tools()),
            "tools/call" => self.call_tool(req.params),
            "ping" => Ok(json!({})),
            other => Err(RpcError::method_not_found(other)),
        };

        Some(match result {
            Ok(v) => protocol::ok(id, v),
            Err(e) => protocol::err(id, e),
        })
    }

    // --- MCP lifecycle ------------------------------------------------------

    fn initialize(&self) -> Value {
        json!({
            "protocolVersion": protocol::PROTOCOL_VERSION,
            "capabilities": { "tools": {} },
            "serverInfo": {
                "name": "hellodb-mcp",
                "version": env!("CARGO_PKG_VERSION"),
                // Current MCP transport model is local single-user execution.
                // Delegation/consent primitives live in hellodb-auth and
                // QueryEngine, but this server currently executes requests as
                // the local identity, not remote principals.
                "auth_model": "single_identity_local_owner",
            }
        })
    }

    fn list_tools(&self) -> Value {
        let tools = serde_json::to_value(tool_catalog()).unwrap_or_else(|_| json!([]));
        json!({ "tools": tools })
    }

    fn call_tool(&self, params: Value) -> Result<Value, RpcError> {
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| RpcError::invalid_params("missing tool name"))?
            .to_string();
        let args = params.get("arguments").cloned().unwrap_or(json!({}));

        let outcome = match name.as_str() {
            "hellodb_identity" => self.tool_identity(),
            "hellodb_list_namespaces" => self.tool_list_namespaces(),
            "hellodb_create_namespace" => self.tool_create_namespace(args),
            "hellodb_register_schema" => self.tool_register_schema(args),
            "hellodb_remember" => self.tool_remember(args),
            "hellodb_note" => self.tool_note(args),
            "hellodb_recall" => self.tool_recall(args),
            "hellodb_query" => self.tool_query(args),
            "hellodb_list_branches" => self.tool_list_branches(args),
            "hellodb_create_branch" => self.tool_create_branch(args),
            "hellodb_merge_branch" => self.tool_merge_branch(args),
            "hellodb_forget" => self.tool_forget(args),
            "hellodb_tail" => self.tool_tail(args),
            "hellodb_reinforce" => self.tool_reinforce(args),
            "hellodb_get_metadata" => self.tool_get_metadata(args),
            "hellodb_archive" => self.tool_archive(args),
            "hellodb_upsert_embedding" => self.tool_upsert_embedding(args),
            "hellodb_recall_deep" => self.tool_recall_deep(args),
            "hellodb_embed" => self.tool_embed(args),
            "hellodb_embed_and_search" => self.tool_embed_and_search(args),
            "hellodb_ingest_text" => self.tool_ingest_text(args),
            "hellodb_find_relevant_memories" => self.tool_find_relevant_memories(args),
            other => Err(format!("unknown tool: {other}")),
        };

        match outcome {
            Ok(value) => Ok(tool_result_ok(&value)),
            Err(msg) => Ok(tool_result_error(&msg)),
        }
    }

    // --- Tool implementations ----------------------------------------------

    fn tool_identity(&self) -> Result<Value, String> {
        Ok(json!({
            "pubkey_b64": self.identity.verifying.to_base64(),
            "fingerprint": self.identity.verifying.fingerprint(),
            "data_dir": crate::identity::data_dir().display().to_string(),
        }))
    }

    fn tool_list_namespaces(&self) -> Result<Value, String> {
        let storage = self.storage.lock().unwrap();
        let ns = storage.list_namespaces().map_err(|e| e.to_string())?;
        Ok(json!(ns
            .into_iter()
            .map(|n| json!({
                "id": n.id,
                "name": n.name,
                "description": n.description,
                "created_at_ms": n.created_at_ms,
                "encrypted": n.encrypted,
                "schemas": n.schemas,
                "is_owner": n.owner == self.identity.verifying,
            }))
            .collect::<Vec<_>>()))
    }

    fn tool_create_namespace(&self, args: Value) -> Result<Value, String> {
        let id = require_string(&args, "id")?;
        let name = require_string(&args, "name")?;
        let description = args
            .get("description")
            .and_then(Value::as_str)
            .map(str::to_string);

        let mut ns = Namespace::new(id.clone(), name, self.identity.verifying.clone(), true);
        ns.description = description;

        let mut storage = self.storage.lock().unwrap();
        storage.create_namespace(ns).map_err(|e| e.to_string())?;
        Ok(json!({ "id": id, "main_branch": format!("{id}/main") }))
    }

    fn tool_register_schema(&self, args: Value) -> Result<Value, String> {
        let schema_id = require_string(&args, "schema_id")?;
        let namespace = require_string(&args, "namespace")?;
        let name = require_string(&args, "name")?;
        let version = args
            .get("version")
            .and_then(Value::as_str)
            .unwrap_or("1.0.0")
            .to_string();
        let fields = args
            .get("fields")
            .and_then(Value::as_array)
            .ok_or_else(|| "missing fields array".to_string())?;

        let parsed_fields: Vec<SchemaField> = fields
            .iter()
            .map(parse_schema_field)
            .collect::<Result<Vec<_>, _>>()?;

        let schema = Schema {
            id: schema_id.clone(),
            version,
            namespace,
            name,
            fields: parsed_fields,
            registered_at_ms: now_ms(),
        };

        let mut storage = self.storage.lock().unwrap();
        storage.register_schema(schema).map_err(|e| e.to_string())?;
        Ok(json!({ "schema_id": schema_id }))
    }

    fn tool_remember(&self, args: Value) -> Result<Value, String> {
        let namespace = require_string(&args, "namespace")?;
        let schema = require_string(&args, "schema")?;
        let data = args
            .get("data")
            .cloned()
            .ok_or_else(|| "missing data".to_string())?;
        let branch = branch_or_default(&args, &namespace);

        let mut storage = self.storage.lock().unwrap();

        // Strict mode: reject writes against unregistered schemas. This keeps
        // typed memory honest — agents can't silently invent schema names and
        // pollute the registry. If you want a looser "just stash this" flow,
        // use `hellodb_note` instead.
        match storage.get_schema(&schema).map_err(|e| e.to_string())? {
            Some(s) if s.namespace == namespace => {}
            Some(s) => {
                return Err(format!(
                    "schema '{}' is registered under namespace '{}', not '{}'. \
                    register the schema in this namespace or use hellodb_note for free-form writes.",
                    schema, s.namespace, namespace
                ));
            }
            None => {
                return Err(format!(
                    "schema '{}' is not registered. call hellodb_register_schema first, \
                    or use hellodb_note for free-form writes that auto-create a '{}.note' schema.",
                    schema, namespace
                ));
            }
        }

        let record = Record::new(&self.identity.signing, schema, namespace, data, None)
            .map_err(|e| e.to_string())?;
        let record_id = record.record_id.clone();

        storage
            .put_record(record, &branch)
            .map_err(|e| e.to_string())?;
        Ok(json!({
            "record_id": record_id,
            "branch": branch,
            "signed_by": self.identity.verifying.fingerprint(),
        }))
    }

    /// Convenience "just stash this" tool. Auto-creates the namespace (owned
    /// by this server) and auto-registers a permissive `{namespace}.note`
    /// schema if missing. Use this for noisy agent writes where you don't
    /// want to design a schema upfront.
    fn tool_note(&self, args: Value) -> Result<Value, String> {
        let namespace = require_string(&args, "namespace")?;
        let data = args
            .get("data")
            .cloned()
            .ok_or_else(|| "missing data".to_string())?;
        let branch = branch_or_default(&args, &namespace);
        let schema_id = format!("{namespace}.note");

        let mut storage = self.storage.lock().unwrap();

        // Ensure namespace exists (idempotent)
        let ns_existed = storage
            .get_namespace(&namespace)
            .map_err(|e| e.to_string())?
            .is_some();
        if !ns_existed {
            let ns = Namespace::new(
                namespace.clone(),
                namespace.clone(),
                self.identity.verifying.clone(),
                true,
            );
            storage.create_namespace(ns).map_err(|e| e.to_string())?;
        }

        // Ensure note schema exists (idempotent, permissive)
        let schema_existed = storage
            .get_schema(&schema_id)
            .map_err(|e| e.to_string())?
            .is_some();
        if !schema_existed {
            let note_schema = Schema {
                id: schema_id.clone(),
                version: "1.0.0".into(),
                namespace: namespace.clone(),
                name: format!("{namespace} note"),
                fields: vec![SchemaField {
                    name: "content".into(),
                    field_type: FieldType::Json,
                    required: false,
                    description: Some("free-form note payload; any JSON shape".into()),
                }],
                registered_at_ms: now_ms(),
            };
            storage
                .register_schema(note_schema)
                .map_err(|e| e.to_string())?;
        }

        let record = Record::new(
            &self.identity.signing,
            schema_id.clone(),
            namespace.clone(),
            data,
            None,
        )
        .map_err(|e| e.to_string())?;
        let record_id = record.record_id.clone();

        storage
            .put_record(record, &branch)
            .map_err(|e| e.to_string())?;
        Ok(json!({
            "record_id": record_id,
            "branch": branch,
            "schema": schema_id,
            "namespace_created": !ns_existed,
            "schema_created": !schema_existed,
            "signed_by": self.identity.verifying.fingerprint(),
        }))
    }

    fn tool_recall(&self, args: Value) -> Result<Value, String> {
        let record_id = require_string(&args, "record_id")?;
        let branch = require_string(&args, "branch")?;

        let storage = self.storage.lock().unwrap();
        match storage
            .get_record(&record_id, &branch)
            .map_err(|e| e.to_string())?
        {
            Some(r) => Ok(record_to_json(&r)),
            None => Err(format!("record not found: {record_id} on branch {branch}")),
        }
    }

    fn tool_query(&self, args: Value) -> Result<Value, String> {
        let namespace = require_string(&args, "namespace")?;
        let branch = branch_or_default(&args, &namespace);

        let mut q = Query::new().namespace(namespace.clone());
        if let Some(schema) = args.get("schema").and_then(Value::as_str) {
            q = q.schema(schema);
        }
        if let Some(limit) = args.get("limit").and_then(Value::as_u64) {
            q = q.limit(limit as usize);
        }
        if let Some(sort_by) = args.get("sort_by").and_then(Value::as_str) {
            let desc = args
                .get("sort_desc")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            q = q.sort(if desc {
                SortField::desc(sort_by)
            } else {
                SortField::asc(sort_by)
            });
        }
        if let Some(filter_val) = args.get("filter") {
            if !filter_val.is_null() {
                let filter = parse_filter(filter_val)?;
                q = q.filter(filter);
            }
        }

        let storage = self.storage.lock().unwrap();
        let engine = QueryEngine::new(&*storage, &self.access);
        let result = engine
            .execute(&q, &self.identity.verifying, &branch, now_ms())
            .map_err(|e| e.to_string())?;

        Ok(json!({
            "records": result.records.iter().map(record_to_json).collect::<Vec<_>>(),
            "total_count": result.total_count,
            "has_more": result.has_more,
        }))
    }

    fn tool_list_branches(&self, args: Value) -> Result<Value, String> {
        let namespace = require_string(&args, "namespace")?;
        let storage = self.storage.lock().unwrap();
        let branches = storage
            .list_branches(&namespace)
            .map_err(|e| e.to_string())?;
        Ok(json!(branches
            .into_iter()
            .map(|b| json!({
                "id": b.id,
                "label": b.label,
                "parent": b.parent,
                "state": format!("{:?}", b.state).to_lowercase(),
                "change_count": b.changes.len(),
                "created_at_ms": b.created_at_ms,
            }))
            .collect::<Vec<_>>()))
    }

    fn tool_create_branch(&self, args: Value) -> Result<Value, String> {
        let namespace = require_string(&args, "namespace")?;
        let label = require_string(&args, "label")?;
        let parent = args
            .get("parent")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| format!("{namespace}/main"));

        let id = format!("{namespace}/{label}");
        let branch = Branch::new(id.clone(), namespace, parent, label);

        let mut storage = self.storage.lock().unwrap();
        storage.create_branch(branch).map_err(|e| e.to_string())?;
        Ok(json!({ "branch_id": id }))
    }

    fn tool_merge_branch(&self, args: Value) -> Result<Value, String> {
        let branch_id = require_string(&args, "branch_id")?;
        let mut storage = self.storage.lock().unwrap();
        let result = storage
            .merge_branch(&branch_id)
            .map_err(|e| e.to_string())?;
        Ok(json!({
            "merged_records": result.merged_records,
            "conflicts": result.conflicts.iter().map(|c| json!({
                "record_id": c.record_id,
                "description": c.description,
            })).collect::<Vec<_>>(),
        }))
    }

    fn tool_forget(&self, args: Value) -> Result<Value, String> {
        let record_id = require_string(&args, "record_id")?;
        let branch = require_string(&args, "branch")?;
        let mut storage = self.storage.lock().unwrap();
        storage
            .delete_record(&record_id, &branch)
            .map_err(|e| e.to_string())?;
        Ok(json!({ "tombstoned": record_id, "branch": branch }))
    }

    /// Reinforce a record — bumps its mutable score + last_reinforced_at.
    /// Doesn't touch the record itself (which is immutable / content-addressed);
    /// metadata lives in a sibling sidecar table.
    fn tool_reinforce(&self, args: Value) -> Result<Value, String> {
        let record_id = require_string(&args, "record_id")?;
        let delta = args.get("delta").and_then(Value::as_f64).unwrap_or(1.0) as f32;
        let now = now_ms();
        let mut storage = self.storage.lock().unwrap();
        let meta = storage
            .reinforce_record(&record_id, delta, now)
            .map_err(|e| e.to_string())?;
        Ok(metadata_to_json(&meta, now, default_half_life_ms()))
    }

    /// Return a record's reinforcement metadata plus its decay-adjusted score.
    /// Callers pass an optional `half_life_days` to control decay speed (default 7).
    fn tool_get_metadata(&self, args: Value) -> Result<Value, String> {
        let record_id = require_string(&args, "record_id")?;
        let half_life_ms = args
            .get("half_life_days")
            .and_then(Value::as_f64)
            .map(|d| (d * 86_400_000.0) as u64)
            .unwrap_or_else(default_half_life_ms);
        let now = now_ms();
        let storage = self.storage.lock().unwrap();
        match storage
            .get_record_metadata(&record_id)
            .map_err(|e| e.to_string())?
        {
            Some(m) => Ok(metadata_to_json(&m, now, half_life_ms)),
            None => Err(format!(
                "no metadata for record {record_id} (never reinforced)"
            )),
        }
    }

    /// Archive a record (soft, reversible — distinct from `hellodb_forget`
    /// which tombstones on a branch). Use for aged-out facts the digest no
    /// longer considers active but shouldn't permanently delete.
    fn tool_archive(&self, args: Value) -> Result<Value, String> {
        let record_id = require_string(&args, "record_id")?;
        let now = now_ms();
        let mut storage = self.storage.lock().unwrap();
        storage
            .archive_record(&record_id, now)
            .map_err(|e| e.to_string())?;
        Ok(json!({ "archived": record_id, "archived_at_ms": now }))
    }

    /// Tail a namespace's write log from a cursor. This is the primitive that
    /// lets a passive observer (memory digest agent, external analytics,
    /// event-driven pipelines) consume new records without touching the
    /// primary writer's hot path.
    fn tool_tail(&self, args: Value) -> Result<Value, String> {
        let namespace = require_string(&args, "namespace")?;
        let after_seq = args.get("after_seq").and_then(Value::as_u64).unwrap_or(0);
        let limit = args
            .get("limit")
            .and_then(Value::as_u64)
            .map(|n| n as usize)
            .unwrap_or(100);
        let branch_filter = args
            .get("branch")
            .and_then(Value::as_str)
            .map(str::to_string);

        let storage = self.storage.lock().unwrap();
        let entries: Vec<TailEntry> = storage
            .tail_records(&namespace, after_seq, limit, branch_filter.as_deref())
            .map_err(|e| e.to_string())?;

        let next_cursor = entries.last().map(|e| e.seq).unwrap_or(after_seq);
        let has_more = entries.len() >= limit;

        Ok(json!({
            "entries": entries.iter().map(|e| json!({
                "seq": e.seq,
                "branch": e.branch,
                "record": record_to_json(&e.record),
            })).collect::<Vec<_>>(),
            "next_cursor": next_cursor,
            "has_more": has_more,
            "count": entries.len(),
        }))
    }

    /// Open (or create) the vector index for a namespace. Cheap: file I/O +
    /// decrypt + in-memory load. Callers should open per-invocation — the
    /// index isn't designed for long-lived concurrent handles.
    fn open_vector_index(&self, namespace: &str) -> Result<VectorIndex, String> {
        let dir = crate::identity::data_dir().join("vectors");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let key = crate::identity::derive_namespace_vector_key(&self.identity.signing, namespace);
        VectorIndex::open(&dir, namespace, &key).map_err(|e| e.to_string())
    }

    /// Store (or overwrite) an embedding for a record_id in the namespace's
    /// vector index. Auto-flushes on mutation per VectorIndex's contract.
    fn tool_upsert_embedding(&self, args: Value) -> Result<Value, String> {
        let namespace = require_string(&args, "namespace")?;
        let record_id = require_string(&args, "record_id")?;
        let embedding = parse_f32_array(&args, "embedding")?;

        let mut index = self.open_vector_index(&namespace)?;
        let dim = embedding.len();
        index
            .upsert(record_id.clone(), embedding)
            .map_err(|e| e.to_string())?;

        Ok(json!({
            "record_id": record_id,
            "namespace": namespace,
            "dim": dim,
            "index_size": index.len(),
        }))
    }

    /// Top-k nearest-neighbor recall joined with storage records and
    /// optional decay-adjusted ranking. Records missing from the branch
    /// (e.g. tombstoned) are skipped rather than errored.
    fn tool_recall_deep(&self, args: Value) -> Result<Value, String> {
        let namespace = require_string(&args, "namespace")?;
        let query_embedding = parse_f32_array(&args, "query_embedding")?;
        let top_k = args
            .get("top_k")
            .and_then(Value::as_u64)
            .map(|n| n as usize)
            .unwrap_or(5);
        let branch = branch_or_default(&args, &namespace);
        let half_life_ms = args
            .get("half_life_days")
            .and_then(Value::as_f64)
            .map(|d| (d * 86_400_000.0) as u64)
            .unwrap_or_else(default_half_life_ms);
        let use_decay = args
            .get("use_decay")
            .and_then(Value::as_bool)
            .unwrap_or(true);

        let index = self.open_vector_index(&namespace)?;
        let index_size = index.len();
        let hits = index
            .search(&query_embedding, top_k)
            .map_err(|e| e.to_string())?;

        let storage = self.storage.lock().unwrap();
        let now = now_ms();

        let mut scored: Vec<(f32, f32, f32, Value)> = Vec::with_capacity(hits.len());
        for hit in hits {
            let record = match storage
                .get_record(&hit.record_id, &branch)
                .map_err(|e| e.to_string())?
            {
                Some(r) => r,
                // Record gone (tombstoned on this branch, or simply not
                // present in this branch's ancestry). Skip silently — the
                // vector index can legitimately outlive a record on a branch.
                None => continue,
            };

            let meta = storage
                .get_record_metadata(&hit.record_id)
                .map_err(|e| e.to_string())?;

            let similarity = hit.score;
            let (decayed, final_score) = match (&meta, use_decay) {
                (Some(m), true) => {
                    let d = decayed_score(m, now, half_life_ms);
                    let clamped = d.clamp(0.0, 10.0);
                    (d, similarity * (1.0 + clamped))
                }
                _ => (0.0, similarity),
            };

            let record_json = record_to_json(&record);
            scored.push((
                final_score,
                similarity,
                decayed,
                json!({
                    "record_id": hit.record_id,
                    "similarity": similarity,
                    "decayed_score": decayed,
                    "final_score": final_score,
                    "record": record_json,
                }),
            ));
        }

        // Re-sort by final_score descending. Inputs are finite (similarity
        // is bounded, decayed_score is clamped), so partial_cmp is safe.
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        let hits_json: Vec<Value> = scored.into_iter().map(|(_, _, _, v)| v).collect();

        Ok(json!({
            "query": {
                "namespace": namespace,
                "top_k": top_k,
                "branch": branch,
                "decay_applied": use_decay,
            },
            "hits": hits_json,
            "index_size": index_size,
        }))
    }

    /// Compute an embedding for the provided text using the configured
    /// backend (via `HELLODB_EMBED_BACKEND` + friends). Returns the vector,
    /// dim, model, and backend so the caller can plug it into
    /// `hellodb_upsert_embedding` or `hellodb_recall_deep` without doing
    /// the HTTP call themselves.
    fn tool_embed(&self, args: Value) -> Result<Value, String> {
        let text = require_string(&args, "text")?;
        let embedder = build_embedder_from_env().map_err(|e| e.to_string())?;
        let v = embedder.embed_one(&text).map_err(|e| e.to_string())?;
        Ok(json!({
            "embedding": v,
            "dim": embedder.dim(),
            "model": embedder.model_id(),
            "backend": embedder.backend_name(),
        }))
    }

    /// Single-call semantic search: embed `query_text` via the configured
    /// backend, then run the same decay-aware ranking as `hellodb_recall_deep`.
    /// This is what agents actually call — no pre-computed vectors required.
    fn tool_embed_and_search(&self, args: Value) -> Result<Value, String> {
        let query_text = require_string(&args, "query_text")?;
        let embedder = build_embedder_from_env().map_err(|e| e.to_string())?;
        let embedding = embedder.embed_one(&query_text).map_err(|e| e.to_string())?;

        // Re-assemble args with the computed embedding + dispatch to recall_deep.
        let mut enriched = args.clone();
        if let Value::Object(ref mut m) = enriched {
            m.insert(
                "query_embedding".into(),
                Value::Array(embedding.into_iter().map(|f| json!(f)).collect()),
            );
        }
        let result = self.tool_recall_deep(enriched)?;

        // Decorate the result with embedder metadata for traceability.
        let decorated = if let Value::Object(mut m) = result {
            m.insert(
                "embedder".into(),
                json!({
                    "backend": embedder.backend_name(),
                    "model": embedder.model_id(),
                    "dim": embedder.dim(),
                    "query_text": query_text,
                }),
            );
            Value::Object(m)
        } else {
            result
        };
        Ok(decorated)
    }

    /// Embed `text` and upsert the vector under `record_id` into the
    /// namespace's vector index. Useful when populating the index without
    /// going through brain (e.g. bulk backfill, or per-record indexing
    /// from an agent's write).
    fn tool_ingest_text(&self, args: Value) -> Result<Value, String> {
        let namespace = require_string(&args, "namespace")?;
        let record_id = require_string(&args, "record_id")?;
        let text = require_string(&args, "text")?;
        let embedder = build_embedder_from_env().map_err(|e| e.to_string())?;
        let v = embedder.embed_one(&text).map_err(|e| e.to_string())?;
        let dim = v.len();
        let mut index = self.open_vector_index(&namespace)?;
        index
            .upsert(record_id.clone(), v)
            .map_err(|e| e.to_string())?;
        Ok(json!({
            "record_id": record_id,
            "namespace": namespace,
            "dim": dim,
            "index_size": index.len(),
            "embedder": {
                "backend": embedder.backend_name(),
                "model": embedder.model_id(),
            }
        }))
    }

    /// Claude Code-shaped memory retrieval. Intentionally distinct from
    /// `hellodb_embed_and_search`:
    ///   - operates on the `claude.memory.*` record shape (type/description/body)
    ///   - returns a flat memory-manifest: one row per memory file
    ///   - filters by memory `type` (user | feedback | project | reference)
    ///   - gracefully degrades when no embedder is configured → keyword
    ///     overlap + reinforcement decay, so it always returns *something*
    ///
    /// This is what a "load top-N relevant memories at turn start" hook
    /// calls. The embed_and_search tool stays around for generic semantic
    /// recall over any namespace; this one speaks the Claude Code memory
    /// dialect directly.
    fn tool_find_relevant_memories(&self, args: Value) -> Result<Value, String> {
        let namespace = require_string(&args, "namespace")?;
        let branch = branch_or_default(&args, &namespace);
        let top_k = args
            .get("top_k")
            .and_then(Value::as_u64)
            .map(|n| n as usize)
            .unwrap_or(5);
        let query_text = args
            .get("query_text")
            .and_then(Value::as_str)
            .map(str::to_string);
        let types_filter: Option<Vec<String>> =
            args.get("types").and_then(Value::as_array).map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            });
        let half_life_ms = args
            .get("half_life_days")
            .and_then(Value::as_f64)
            .map(|d| (d * 86_400_000.0) as u64)
            .unwrap_or_else(default_half_life_ms);

        // Try the vector path first: only when the caller gave us query_text
        // AND the embedder env is configured. Any failure here (no embedder,
        // empty index, dim mismatch) falls through to the keyword path rather
        // than erroring — retrieval should degrade gracefully, not refuse.
        let mut used_strategy = "keyword+decay";
        let mut vector_hits: Vec<(String, f32)> = Vec::new();
        if let Some(ref qt) = query_text {
            if let Ok(embedder) = build_embedder_from_env() {
                if let Ok(embedding) = embedder.embed_one(qt) {
                    if let Ok(index) = self.open_vector_index(&namespace) {
                        if !index.is_empty() {
                            // Ask for more hits than top_k so type-filtering
                            // still has material to work with.
                            let wanted = (top_k * 4).max(20);
                            if let Ok(hits) = index.search(&embedding, wanted) {
                                vector_hits =
                                    hits.into_iter().map(|h| (h.record_id, h.score)).collect();
                                if !vector_hits.is_empty() {
                                    used_strategy = "vector+decay";
                                }
                            }
                        }
                    }
                }
            }
        }

        let storage = self.storage.lock().unwrap();
        let now = now_ms();

        // Collect candidate records. In vector mode, fetch just the hit ids.
        // In keyword mode, scan the namespace on the target branch (bounded
        // by QueryEngine's default limit — fine for memory files, which are
        // dozens, not millions, per project).
        let candidates: Vec<(Record, Option<f32>)> = if used_strategy == "vector+decay" {
            let mut out = Vec::with_capacity(vector_hits.len());
            for (rid, sim) in &vector_hits {
                if let Ok(Some(r)) = storage.get_record(rid, &branch) {
                    out.push((r, Some(*sim)));
                }
            }
            out
        } else {
            let q = Query::new().namespace(namespace.clone()).limit(1000);
            let engine = QueryEngine::new(&*storage, &self.access);
            let result = engine
                .execute(&q, &self.identity.verifying, &branch, now)
                .map_err(|e| e.to_string())?;
            result.records.into_iter().map(|r| (r, None)).collect()
        };

        // Lowercase tokenized query for keyword overlap scoring. Crude but
        // good enough when there's no embedder — splits on non-alphanumeric,
        // drops tokens shorter than 3 chars (noise).
        let query_tokens: Vec<String> = query_text
            .as_deref()
            .map(|q| {
                q.to_lowercase()
                    .split(|c: char| !c.is_alphanumeric())
                    .filter(|t| t.len() >= 3)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();

        let mut scored: Vec<(f32, Value)> = Vec::with_capacity(candidates.len());
        for (record, similarity) in candidates {
            // Apply type filter BEFORE scoring — skip records whose
            // `data.type` isn't in the allowlist when one is provided.
            let mem_type = record
                .data
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            if let Some(ref allow) = types_filter {
                if !allow.iter().any(|t| t == &mem_type) {
                    continue;
                }
            }

            let description = record
                .data
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let body = record
                .data
                .get("body")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let source_path = record
                .data
                .get("source_path")
                .and_then(Value::as_str)
                .map(str::to_string);
            let project = record
                .data
                .get("project")
                .and_then(Value::as_str)
                .map(str::to_string);
            let mtime_ms = record.data.get("mtime_ms").and_then(Value::as_u64);

            // Decayed reinforcement score — flat 0 if the record was never
            // reinforced. The +1 keeps un-reinforced records competitive
            // rather than zeroing them out entirely.
            let decayed = storage
                .get_record_metadata(&record.record_id)
                .ok()
                .flatten()
                .map(|m| decayed_score(&m, now, half_life_ms))
                .unwrap_or(0.0);

            // Keyword overlap: fraction of query tokens present in the
            // haystack (description + body). Cheap and deterministic.
            let keyword_score = if query_tokens.is_empty() {
                0.0
            } else {
                let hay = format!("{description} {body}").to_lowercase();
                let hits = query_tokens
                    .iter()
                    .filter(|t| hay.contains(t.as_str()))
                    .count();
                hits as f32 / query_tokens.len() as f32
            };

            // Final score composition:
            //   - vector mode: similarity * (1 + decay)
            //   - keyword mode: (keyword_score + 0.1) * (1 + decay)
            //     (the 0.1 offset keeps recall from collapsing to zero
            //     when the caller passed no query_text)
            let final_score = match similarity {
                Some(sim) => sim * (1.0 + decayed.clamp(0.0, 10.0)),
                None => (keyword_score + 0.1) * (1.0 + decayed.clamp(0.0, 10.0)),
            };

            // Pick a "statement" — the one-line summary a caller actually
            // wants to show in a top-N list. Prefer description; fall back
            // to the body's first non-empty line; finally, source_path.
            let statement = if !description.is_empty() {
                description.clone()
            } else {
                body.lines()
                    .map(str::trim)
                    .find(|l| !l.is_empty())
                    .map(str::to_string)
                    .unwrap_or_else(|| source_path.clone().unwrap_or_default())
            };

            let mut item = json!({
                "record_id": record.record_id,
                "type": mem_type,
                "description": description,
                "statement": statement,
                "source_path": source_path,
                "project": project,
                "mtime_ms": mtime_ms,
                "decayed_score": decayed,
                "final_score": final_score,
                "keyword_score": keyword_score,
            });
            if let Some(sim) = similarity {
                item["similarity"] = json!(sim);
            }
            scored.push((final_score, item));
        }

        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        let memories: Vec<Value> = scored.into_iter().take(top_k).map(|(_, v)| v).collect();

        Ok(json!({
            "memories": memories,
            "namespace": namespace,
            "branch": branch,
            "strategy": used_strategy,
            "top_k": top_k,
            "query_text": query_text,
            "types_filter": types_filter,
        }))
    }
}

// --- Helpers ---------------------------------------------------------------

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

/// Default half-life for score decay: 7 days in ms.
fn default_half_life_ms() -> u64 {
    7 * 86_400_000
}

fn metadata_to_json(meta: &RecordMetadata, now_ms: u64, half_life_ms: u64) -> Value {
    json!({
        "record_id": meta.record_id,
        "score": meta.score,
        "decayed_score": decayed_score(meta, now_ms, half_life_ms),
        "reinforce_count": meta.reinforce_count,
        "last_reinforced_at_ms": meta.last_reinforced_at_ms,
        "first_seen_ms": meta.first_seen_ms,
        "archived_at_ms": meta.archived_at_ms,
        "half_life_ms": half_life_ms,
    })
}

/// Parse a required JSON array of numbers as `Vec<f32>`. Errors cleanly if
/// the key is missing, isn't an array, or any element isn't a finite number.
fn parse_f32_array(args: &Value, key: &str) -> Result<Vec<f32>, String> {
    let arr = args
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("missing array argument: {key}"))?;
    let mut out = Vec::with_capacity(arr.len());
    for (i, v) in arr.iter().enumerate() {
        let n = v
            .as_f64()
            .ok_or_else(|| format!("{key}[{i}] is not a number"))?;
        out.push(n as f32);
    }
    Ok(out)
}

fn require_string(args: &Value, key: &str) -> Result<String, String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("missing string argument: {key}"))
}

fn branch_or_default(args: &Value, namespace: &str) -> String {
    args.get("branch")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| format!("{namespace}/main"))
}

fn parse_schema_field(v: &Value) -> Result<SchemaField, String> {
    let name = v
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| "schema field missing name".to_string())?
        .to_string();
    let type_str = v
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("schema field '{name}' missing type"))?;
    let required = v.get("required").and_then(Value::as_bool).unwrap_or(false);
    let description = v
        .get("description")
        .and_then(Value::as_str)
        .map(str::to_string);
    let field_type = match type_str {
        "string" => FieldType::String,
        "integer" => FieldType::Integer,
        "float" => FieldType::Float,
        "boolean" => FieldType::Boolean,
        "timestamp" => FieldType::Timestamp,
        "blob" => FieldType::Blob,
        "json" => FieldType::Json,
        other => return Err(format!("unsupported field type: {other}")),
    };
    Ok(SchemaField {
        name,
        field_type,
        required,
        description,
    })
}

/// Parse a simple filter JSON: either
///   { "op": "eq" | "ne" | "gt" | "lt" | "gte" | "lte" | "contains" | "starts_with",
///     "field": "...", "value": ... }
/// or
///   { "op": "and" | "or", "filters": [ <filter>, ... ] }
/// or
///   { "op": "not", "filter": <filter> }
fn parse_filter(v: &Value) -> Result<Filter, String> {
    let op = v
        .get("op")
        .and_then(Value::as_str)
        .ok_or_else(|| "filter missing op".to_string())?;

    match op {
        "and" | "or" => {
            let arr = v
                .get("filters")
                .and_then(Value::as_array)
                .ok_or_else(|| format!("{op} filter requires 'filters' array"))?;
            let children = arr
                .iter()
                .map(parse_filter)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(if op == "and" {
                Filter::And(children)
            } else {
                Filter::Or(children)
            })
        }
        "not" => {
            let inner = v
                .get("filter")
                .ok_or_else(|| "not filter requires 'filter'".to_string())?;
            Ok(Filter::Not(Box::new(parse_filter(inner)?)))
        }
        _ => {
            let field = v
                .get("field")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("{op} filter requires 'field'"))?
                .to_string();
            let value = v
                .get("value")
                .cloned()
                .ok_or_else(|| format!("{op} filter requires 'value'"))?;
            match op {
                "eq" => Ok(Filter::Eq(field, value)),
                "ne" => Ok(Filter::Ne(field, value)),
                "gt" => Ok(Filter::Gt(field, value)),
                "lt" => Ok(Filter::Lt(field, value)),
                "gte" => Ok(Filter::Gte(field, value)),
                "lte" => Ok(Filter::Lte(field, value)),
                "contains" => {
                    let s = value
                        .as_str()
                        .ok_or("contains value must be string")?
                        .to_string();
                    Ok(Filter::Contains(field, s))
                }
                "starts_with" => {
                    let s = value
                        .as_str()
                        .ok_or("starts_with value must be string")?
                        .to_string();
                    Ok(Filter::StartsWith(field, s))
                }
                other => Err(format!("unknown filter op: {other}")),
            }
        }
    }
}

fn record_to_json(r: &Record) -> Value {
    json!({
        "record_id": r.record_id,
        "schema": r.schema,
        "namespace": r.namespace,
        "created_by": r.created_by.to_base64(),
        "created_at_ms": r.created_at_ms,
        "previous_version": r.previous_version,
        "data": r.data,
    })
}

/// Wrap a success value in the MCP tools/call content envelope.
fn tool_result_ok(value: &Value) -> Value {
    let text = serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string());
    json!({
        "content": [{ "type": "text", "text": text }],
        "isError": false,
    })
}

/// Tool-level error (returned as a successful JSON-RPC response with isError=true,
/// per MCP spec). Keeps the channel open for the client to surface the message.
fn tool_result_error(msg: &str) -> Value {
    json!({
        "content": [{ "type": "text", "text": msg }],
        "isError": true,
    })
}

// --- Tool catalog ----------------------------------------------------------

#[derive(Serialize)]
struct ToolDef {
    name: &'static str,
    description: &'static str,
    #[serde(rename = "inputSchema")]
    input_schema: Value,
}

fn tool_catalog() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "hellodb_identity",
            description: "Return this MCP server's signing identity (public key + fingerprint) and data directory. Use this to confirm the server is wired up and to see which key will sign records.",
            input_schema: json!({ "type": "object", "properties": {} }),
        },
        ToolDef {
            name: "hellodb_list_namespaces",
            description: "List all namespaces in this sovereign store. Each namespace is an isolated data domain with its own schemas, records, and branches.",
            input_schema: json!({ "type": "object", "properties": {} }),
        },
        ToolDef {
            name: "hellodb_create_namespace",
            description: "Create a new namespace owned by this server's identity. Auto-creates a 'main' branch. Use reverse-domain IDs like 'claude.memory' or 'personal.journal'.",
            input_schema: json!({
                "type": "object",
                "required": ["id", "name"],
                "properties": {
                    "id": { "type": "string", "description": "Reverse-domain namespace id (e.g. 'claude.memory')" },
                    "name": { "type": "string", "description": "Human-readable name" },
                    "description": { "type": "string" }
                }
            }),
        },
        ToolDef {
            name: "hellodb_register_schema",
            description: "Register a typed schema within a namespace. Records written to this schema must match its field definitions.",
            input_schema: json!({
                "type": "object",
                "required": ["schema_id", "namespace", "name", "fields"],
                "properties": {
                    "schema_id": { "type": "string", "description": "e.g. 'claude.memory.feedback'" },
                    "namespace": { "type": "string" },
                    "name": { "type": "string" },
                    "version": { "type": "string", "default": "1.0.0" },
                    "fields": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "required": ["name", "type"],
                            "properties": {
                                "name": { "type": "string" },
                                "type": { "type": "string", "enum": ["string","integer","float","boolean","timestamp","blob","json"] },
                                "required": { "type": "boolean", "default": false },
                                "description": { "type": "string" }
                            }
                        }
                    }
                }
            }),
        },
        ToolDef {
            name: "hellodb_remember",
            description: "Write a signed, content-addressed record to a branch (default: '{namespace}/main'). Strict: the schema MUST be registered in the target namespace first via hellodb_register_schema — otherwise this call errors. For free-form writes without upfront schema design, use hellodb_note instead. Returns the record_id for later recall.",
            input_schema: json!({
                "type": "object",
                "required": ["namespace", "schema", "data"],
                "properties": {
                    "namespace": { "type": "string" },
                    "schema": { "type": "string", "description": "Fully qualified schema id already registered in this namespace" },
                    "data": { "type": "object", "description": "Record data conforming to the schema" },
                    "branch": { "type": "string", "description": "Target branch id (default: {namespace}/main)" }
                }
            }),
        },
        ToolDef {
            name: "hellodb_note",
            description: "Convenience 'just stash this' write. Auto-creates the namespace (owned by this server) and auto-registers a permissive '{namespace}.note' schema on first use — so one call is enough to start capturing memory with no setup. Prefer this for quick agent writes; use hellodb_remember + register_schema when you want typed memory.",
            input_schema: json!({
                "type": "object",
                "required": ["namespace", "data"],
                "properties": {
                    "namespace": { "type": "string", "description": "Reverse-domain namespace id; created if missing" },
                    "data": { "type": "object", "description": "Any JSON payload — no shape enforced" },
                    "branch": { "type": "string", "description": "Target branch id (default: {namespace}/main)" }
                }
            }),
        },
        ToolDef {
            name: "hellodb_recall",
            description: "Fetch a record by its content hash from a specific branch (branches inherit from parents).",
            input_schema: json!({
                "type": "object",
                "required": ["record_id", "branch"],
                "properties": {
                    "record_id": { "type": "string" },
                    "branch": { "type": "string" }
                }
            }),
        },
        ToolDef {
            name: "hellodb_query",
            description: "Query records with optional schema filter, field predicate, sort, and limit. Filter shape: {op: 'eq'|'ne'|'gt'|'lt'|'gte'|'lte'|'contains'|'starts_with', field, value} or {op:'and'|'or', filters:[...]} or {op:'not', filter:{...}}.",
            input_schema: json!({
                "type": "object",
                "required": ["namespace"],
                "properties": {
                    "namespace": { "type": "string" },
                    "schema": { "type": "string" },
                    "branch": { "type": "string", "description": "Default: {namespace}/main" },
                    "filter": { "type": "object" },
                    "sort_by": { "type": "string", "description": "Field path to sort by" },
                    "sort_desc": { "type": "boolean", "default": false },
                    "limit": { "type": "integer", "default": 100 }
                }
            }),
        },
        ToolDef {
            name: "hellodb_list_branches",
            description: "List all branches (including main, drafts, and merged) for a namespace.",
            input_schema: json!({
                "type": "object",
                "required": ["namespace"],
                "properties": { "namespace": { "type": "string" } }
            }),
        },
        ToolDef {
            name: "hellodb_create_branch",
            description: "Create a draft branch off a parent (default: main). Use for agent scratch work that needs human review before merging.",
            input_schema: json!({
                "type": "object",
                "required": ["namespace", "label"],
                "properties": {
                    "namespace": { "type": "string" },
                    "label": { "type": "string" },
                    "parent": { "type": "string", "description": "Default: {namespace}/main" }
                }
            }),
        },
        ToolDef {
            name: "hellodb_merge_branch",
            description: "Merge a branch into its parent. Returns merged record ids and any conflicts. This is the 'approve agent writes' step.",
            input_schema: json!({
                "type": "object",
                "required": ["branch_id"],
                "properties": { "branch_id": { "type": "string" } }
            }),
        },
        ToolDef {
            name: "hellodb_forget",
            description: "Tombstone a record on a branch (hides it without rewriting history).",
            input_schema: json!({
                "type": "object",
                "required": ["record_id", "branch"],
                "properties": {
                    "record_id": { "type": "string" },
                    "branch": { "type": "string" }
                }
            }),
        },
        ToolDef {
            name: "hellodb_reinforce",
            description: "Signal that a record is useful/relevant again. Bumps its mutable score by `delta` (default 1.0), increments reinforce_count, and stamps last_reinforced_at. Record content is unchanged — metadata lives in a sidecar table. Use this in the digest/consolidation loop: when a new episode corroborates an existing fact, reinforce the fact.",
            input_schema: json!({
                "type": "object",
                "required": ["record_id"],
                "properties": {
                    "record_id": { "type": "string" },
                    "delta": { "type": "number", "default": 1.0, "description": "Amount to add to score (can be negative to demote)" }
                }
            }),
        },
        ToolDef {
            name: "hellodb_get_metadata",
            description: "Fetch a record's reinforcement metadata, including the time-decayed score. Default half-life is 7 days; pass `half_life_days` to override. Use during recall to rank candidates by how 'fresh' their evidence is.",
            input_schema: json!({
                "type": "object",
                "required": ["record_id"],
                "properties": {
                    "record_id": { "type": "string" },
                    "half_life_days": { "type": "number", "default": 7.0 }
                }
            }),
        },
        ToolDef {
            name: "hellodb_archive",
            description: "Mark a record as archived — soft, reversible, distinct from hellodb_forget (which tombstones on a branch). Archived records stay in storage with their content hash intact but are typically hidden from recall.",
            input_schema: json!({
                "type": "object",
                "required": ["record_id"],
                "properties": { "record_id": { "type": "string" } }
            }),
        },
        ToolDef {
            name: "hellodb_tail",
            description: "Tail a namespace's monotonic write log from a cursor. Returns up to `limit` new entries with seq > after_seq, each including its seq, branch, and full Record. Pass the response's `next_cursor` back as `after_seq` on the next call to resume. This is the primitive that enables passive memory pipelines, event-driven digesters, and out-of-hot-path observers — the primary agent writes; a sidecar tails and processes.",
            input_schema: json!({
                "type": "object",
                "required": ["namespace"],
                "properties": {
                    "namespace": { "type": "string" },
                    "after_seq": { "type": "integer", "default": 0, "description": "Resume cursor; 0 to start from the beginning" },
                    "limit": { "type": "integer", "default": 100 },
                    "branch": { "type": "string", "description": "Optional: only entries on this exact branch" }
                }
            }),
        },
        ToolDef {
            name: "hellodb_upsert_embedding",
            description: "Store or overwrite an embedding vector for a record_id in the namespace's on-disk vector index. Embeddings are supplied as a JSON array of numbers (cast to f32); this server does not generate them. The index is sealed at rest with a per-namespace key derived from the server's identity, and auto-flushes on every upsert. The first upsert in a namespace pins the dimensionality; later upserts must match. Returns the record_id, namespace, embedding dim, and the total index size.",
            input_schema: json!({
                "type": "object",
                "required": ["namespace", "record_id", "embedding"],
                "properties": {
                    "namespace": { "type": "string" },
                    "record_id": { "type": "string", "description": "Content hash of the record this embedding represents" },
                    "embedding": {
                        "type": "array",
                        "items": { "type": "number" },
                        "description": "Dense embedding vector (f32). Must be non-empty and match the namespace's pinned dim after the first upsert."
                    }
                }
            }),
        },
        ToolDef {
            name: "hellodb_recall_deep",
            description: "Semantic recall: top-k nearest-neighbor search over the namespace's vector index, joined with the actual records from storage and optionally reranked by reinforcement decay. Missing records (tombstoned or not present on the target branch) are skipped silently. Final score = similarity * (1 + clamp(decayed_score, 0, 10)) when decay is applied; otherwise similarity alone. Default half-life is 7 days. Use this as the default recall path when you have an embedding of the query; fall back to hellodb_query for predicate-based search.",
            input_schema: json!({
                "type": "object",
                "required": ["namespace", "query_embedding"],
                "properties": {
                    "namespace": { "type": "string" },
                    "query_embedding": {
                        "type": "array",
                        "items": { "type": "number" },
                        "description": "Query embedding (f32); must match the namespace's pinned dim."
                    },
                    "top_k": { "type": "integer", "default": 5 },
                    "branch": { "type": "string", "description": "Default: {namespace}/main" },
                    "half_life_days": { "type": "number", "default": 7.0 },
                    "use_decay": { "type": "boolean", "default": true }
                }
            }),
        },
        ToolDef {
            name: "hellodb_embed",
            description: "Compute an embedding for the given text using the configured backend (set HELLODB_EMBED_BACKEND to 'cloudflare' | 'openai' | 'huggingface' | 'mock' | 'fastembed'). Returns the vector + dim + model. Use this if you want the embedding to hand off to hellodb_upsert_embedding or hellodb_recall_deep yourself; otherwise prefer hellodb_embed_and_search.",
            input_schema: json!({
                "type": "object",
                "required": ["text"],
                "properties": {
                    "text": { "type": "string" }
                }
            }),
        },
        ToolDef {
            name: "hellodb_embed_and_search",
            description: "Semantic recall in one call: embeds `query_text` via the configured backend (HELLODB_EMBED_BACKEND) and runs the same decay-aware ranking as hellodb_recall_deep. This is the default recall path for agents — no pre-computed vectors required.",
            input_schema: json!({
                "type": "object",
                "required": ["namespace", "query_text"],
                "properties": {
                    "namespace": { "type": "string" },
                    "query_text": { "type": "string" },
                    "top_k": { "type": "integer", "default": 5 },
                    "branch": { "type": "string", "description": "Default: {namespace}/main" },
                    "half_life_days": { "type": "number", "default": 7.0 },
                    "use_decay": { "type": "boolean", "default": true }
                }
            }),
        },
        ToolDef {
            name: "hellodb_ingest_text",
            description: "Embed `text` and upsert the resulting vector under `record_id` into the namespace's vector index. Lets agents populate semantic search without having to compute embeddings themselves. (Brain does this automatically during digest for its facts; this tool lets you do it manually for episodes or ad-hoc records.)",
            input_schema: json!({
                "type": "object",
                "required": ["namespace", "record_id", "text"],
                "properties": {
                    "namespace": { "type": "string" },
                    "record_id": { "type": "string" },
                    "text": { "type": "string" }
                }
            }),
        },
        ToolDef {
            name: "hellodb_find_relevant_memories",
            description: "Return the top-k relevant Claude Code memory files in a namespace. Mirrors Claude Code's memory-manifest retrieval: each hit carries type (user|feedback|project|reference), description, statement, source_path, project, mtime_ms, plus decayed_score and final_score. Hybrid ranking: if an embedder is configured (HELLODB_EMBED_BACKEND) and `query_text` is provided, uses vector similarity; otherwise falls back to keyword overlap + reinforcement decay. Always degrades gracefully — never errors for 'no embedder', just returns the best keyword+decay ranking available. Populated by `hellodb ingest --from-claudemd` or manual writes to a claude.memory.* namespace.",
            input_schema: json!({
                "type": "object",
                "required": ["namespace"],
                "properties": {
                    "namespace": { "type": "string", "description": "e.g. 'claude.memory.hellodb' — the per-project namespace populated by `hellodb ingest`" },
                    "query_text": { "type": "string", "description": "Free-form query. If a vector embedder is configured, used for semantic similarity; otherwise tokenized for keyword overlap scoring." },
                    "top_k": { "type": "integer", "default": 5 },
                    "types": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Optional allowlist of Claude memory types to include: user | feedback | project | reference"
                    },
                    "branch": { "type": "string", "description": "Default: {namespace}/main" },
                    "half_life_days": { "type": "number", "default": 7.0 }
                }
            }),
        },
    ]
}
