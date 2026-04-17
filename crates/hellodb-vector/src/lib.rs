//! hellodb Vector Index
//!
//! Per-namespace flat vector index for semantic recall. Embeddings are
//! supplied by the caller as `&[f32]`; this crate does not generate them.
//! The serialized index is sealed at rest with a [`hellodb_crypto::NamespaceKey`],
//! matching the rest of the hellodb storage model.
//!
//! Scale target is personal-memory: thousands of vectors per namespace.
//! Current implementation is a brute-force cosine-similarity scan.
//! TODO: HNSW for larger namespaces.

pub mod error;
pub mod index;
pub mod math;

pub use error::VectorError;
pub use index::{SearchHit, VectorIndex};
