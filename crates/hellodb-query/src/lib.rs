//! hellodb-query — on-device query engine for hellodb.
//!
//! The "inverse SQL warehouse": instead of a cloud cluster processing your
//! query, the query engine runs entirely on YOUR device against YOUR data.
//!
//! # Architecture
//!
//! ```text
//! Query::new()
//!     .schema("commerce.listing")
//!     .filter(Filter::Gt("price", 20.0))
//!     .sort(SortField::desc("price"))
//!     .limit(50)
//!         │
//!         ▼
//!   QueryEngine::execute()
//!     1. Access check (via AccessGate)
//!     2. Fetch candidates (via StorageEngine indexes)
//!     3. Apply filter tree
//!     4. Multi-field sort
//!     5. Cursor/offset pagination
//!         │
//!         ▼
//!   QueryResult { records, total_count, next_cursor, has_more }
//! ```

pub mod cursor;
pub mod engine;
pub mod error;
pub mod filter;
pub mod query;
pub mod sort;

// Re-export primary types for ergonomic use
pub use cursor::Cursor;
pub use engine::QueryEngine;
pub use error::QueryError;
pub use filter::Filter;
pub use query::{Query, QueryResult, MAX_QUERY_LIMIT};
pub use sort::{SortField, SortOrder};
