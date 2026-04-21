//! Content-addressed record model.
//!
//! Every record in hellodb gets a BLAKE3 content hash as its ID.
//! Records are signed by the writing app's identity key.
//! Records belong to a namespace and conform to a registered schema.

use hellodb_crypto::{content_hash, Signature, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};

use crate::canonical::canonicalize_value;
use crate::error::CoreError;

/// Content-addressed record ID (BLAKE3 hex string).
pub type RecordId = String;

/// Hard cap on a record's canonical payload size in bytes.
///
/// hellodb is a memory store for agent context, not a blob store. Once a
/// single record crosses this threshold it stops being something a
/// retrieval tool can sensibly hand to an LLM turn and starts being a
/// context-stuffing vector in its own right (the MCP caller can pull that
/// record by id and dump it straight into the window). The cap sits at
/// the write boundary so by the time any retrieval path touches the data
/// it is already bounded.
///
/// 256 KiB is ~64k tokens at typical English density — generous for a
/// single fact, prohibitive for a raw transcript dump. Callers that need
/// to archive larger material should slice it into multiple records and
/// reference them by id, which is what the digest pipeline does anyway.
pub const MAX_RECORD_PAYLOAD_BYTES: usize = 256 * 1024;

/// A signed, content-addressed hellodb record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Record {
    /// Content hash of the canonical signed payload. Computed, not user-set.
    pub record_id: RecordId,
    /// Schema identifier (e.g., "ainp.commerce.listing", "health.vitals").
    pub schema: String,
    /// The namespace this record belongs to.
    pub namespace: String,
    /// Public key of the app/agent that created this record.
    pub created_by: VerifyingKey,
    /// Unix timestamp in milliseconds when created.
    pub created_at_ms: u64,
    /// The actual data (schema-conformant JSON).
    pub data: serde_json::Value,
    /// Optional reference to a previous version of this record.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_version: Option<RecordId>,
    /// Ed25519 signature over canonical payload.
    pub sig: Signature,
}

/// Intermediate for signing (record without record_id and sig).
#[derive(Serialize)]
struct RecordForSigning<'a> {
    schema: &'a str,
    namespace: &'a str,
    created_by: &'a VerifyingKey,
    created_at_ms: u64,
    data: &'a serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    previous_version: &'a Option<RecordId>,
}

impl Record {
    /// Create and sign a new record (uses current system time).
    pub fn new(
        signing_key: &SigningKey,
        schema: String,
        namespace: String,
        data: serde_json::Value,
        previous_version: Option<RecordId>,
    ) -> Result<Self, CoreError> {
        let created_at_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        Self::new_with_timestamp(
            signing_key,
            schema,
            namespace,
            data,
            previous_version,
            created_at_ms,
        )
    }

    /// Create a record with an explicit timestamp (for testing/import).
    pub fn new_with_timestamp(
        signing_key: &SigningKey,
        schema: String,
        namespace: String,
        data: serde_json::Value,
        previous_version: Option<RecordId>,
        created_at_ms: u64,
    ) -> Result<Self, CoreError> {
        let created_by = signing_key.verifying_key();

        let signable = RecordForSigning {
            schema: &schema,
            namespace: &namespace,
            created_by: &created_by,
            created_at_ms,
            data: &data,
            previous_version: &previous_version,
        };

        let canonical = canonicalize_value(&signable)?;
        if canonical.len() > MAX_RECORD_PAYLOAD_BYTES {
            return Err(CoreError::PayloadTooLarge {
                size: canonical.len(),
                limit: MAX_RECORD_PAYLOAD_BYTES,
            });
        }
        let record_id = content_hash(&canonical);
        let sig = signing_key.sign(&canonical);

        Ok(Self {
            record_id,
            schema,
            namespace,
            created_by,
            created_at_ms,
            data,
            previous_version,
            sig,
        })
    }

    /// Verify the record's signature and content hash integrity.
    pub fn verify(&self) -> Result<(), CoreError> {
        let signable = RecordForSigning {
            schema: &self.schema,
            namespace: &self.namespace,
            created_by: &self.created_by,
            created_at_ms: self.created_at_ms,
            data: &self.data,
            previous_version: &self.previous_version,
        };

        let canonical = canonicalize_value(&signable)?;

        // Verify content hash
        let expected_id = content_hash(&canonical);
        if expected_id != self.record_id {
            return Err(CoreError::InvalidRecord(format!(
                "record_id mismatch: expected {}, got {}",
                expected_id, self.record_id
            )));
        }

        // Verify signature
        self.created_by
            .verify(&canonical, &self.sig)
            .map_err(CoreError::Crypto)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hellodb_crypto::KeyPair;
    use serde_json::json;

    #[test]
    fn create_and_verify() {
        let kp = KeyPair::generate();
        let rec = Record::new(
            &kp.signing,
            "ainp.commerce.listing".into(),
            "ainp.commerce".into(),
            json!({"title": "Rust Engineer", "remote": true}),
            None,
        )
        .unwrap();

        assert!(rec.verify().is_ok());
        assert!(!rec.record_id.is_empty());
        assert_eq!(rec.schema, "ainp.commerce.listing");
        assert_eq!(rec.namespace, "ainp.commerce");
    }

    #[test]
    fn tampered_data_fails() {
        let kp = KeyPair::generate();
        let mut rec = Record::new(
            &kp.signing,
            "test.schema".into(),
            "test".into(),
            json!({"title": "Rust Engineer"}),
            None,
        )
        .unwrap();

        rec.data = json!({"title": "Go Engineer"});
        assert!(rec.verify().is_err());
    }

    #[test]
    fn tampered_id_fails() {
        let kp = KeyPair::generate();
        let mut rec = Record::new(
            &kp.signing,
            "test.schema".into(),
            "test".into(),
            json!({"title": "Rust Engineer"}),
            None,
        )
        .unwrap();

        rec.record_id = "0000000000000000".into();
        assert!(rec.verify().is_err());
    }

    #[test]
    fn versioning() {
        let kp = KeyPair::generate();
        let v1 = Record::new(
            &kp.signing,
            "test.schema".into(),
            "test".into(),
            json!({"title": "Rust dev", "version": 1}),
            None,
        )
        .unwrap();

        let v2 = Record::new(
            &kp.signing,
            "test.schema".into(),
            "test".into(),
            json!({"title": "Senior Rust dev", "version": 2}),
            Some(v1.record_id.clone()),
        )
        .unwrap();

        assert!(v2.verify().is_ok());
        assert_eq!(v2.previous_version.as_ref().unwrap(), &v1.record_id);
    }

    #[test]
    fn serialization_roundtrip() {
        let kp = KeyPair::generate();
        let rec = Record::new(
            &kp.signing,
            "health.vitals".into(),
            "health".into(),
            json!({"heart_rate": 72, "timestamp_ms": 1000}),
            None,
        )
        .unwrap();

        let json_str = serde_json::to_string(&rec).unwrap();
        let restored: Record = serde_json::from_str(&json_str).unwrap();
        assert!(restored.verify().is_ok());
        assert_eq!(restored.record_id, rec.record_id);
    }

    #[test]
    fn oversize_payload_is_rejected() {
        let kp = KeyPair::generate();
        // One field whose value alone blows past the cap. We use a String
        // because serde_json::Value::String gets escaped 1:1 in canonical
        // form, so the byte count is predictable.
        let big = "x".repeat(MAX_RECORD_PAYLOAD_BYTES + 1_024);
        let result = Record::new(
            &kp.signing,
            "test.schema".into(),
            "test".into(),
            json!({ "blob": big }),
            None,
        );
        match result {
            Err(CoreError::PayloadTooLarge { size, limit }) => {
                assert!(size > limit);
                assert_eq!(limit, MAX_RECORD_PAYLOAD_BYTES);
            }
            other => panic!("expected PayloadTooLarge, got {other:?}"),
        }
    }

    #[test]
    fn deterministic_with_timestamp() {
        let kp = KeyPair::generate();
        let r1 = Record::new_with_timestamp(
            &kp.signing,
            "test.schema".into(),
            "test".into(),
            json!({"value": 42}),
            None,
            1000,
        )
        .unwrap();

        let r2 = Record::new_with_timestamp(
            &kp.signing,
            "test.schema".into(),
            "test".into(),
            json!({"value": 42}),
            None,
            1000,
        )
        .unwrap();

        assert_eq!(r1.record_id, r2.record_id);
    }
}
