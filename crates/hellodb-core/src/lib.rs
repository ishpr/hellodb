//! hellodb Core
//!
//! Defines the record model, schema registry, namespace isolation,
//! branch metadata, and canonicalization rules for the hellodb
//! sovereign data layer.

pub mod branch;
pub mod canonical;
pub mod error;
pub mod namespace;
pub mod record;
pub mod schema;

pub use branch::{Branch, BranchId, BranchState, MergeConflict, MergeResult};
pub use canonical::{canonicalize, canonicalize_value};
pub use error::CoreError;
pub use namespace::{Namespace, NamespaceId};
pub use record::{Record, RecordId, MAX_RECORD_PAYLOAD_BYTES};
pub use schema::{FieldType, Schema, SchemaField, SchemaRegistry};
