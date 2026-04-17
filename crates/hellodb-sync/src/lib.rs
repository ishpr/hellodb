//! hellodb-sync — encrypted delta sync to personal cloud storage.
//!
//! The "inverse Delta Lake": instead of corporate cloud ingestion,
//! data syncs encrypted to YOUR personal cloud bucket.
//!
//! # Architecture
//!
//! ```text
//! Device A                    Personal Cloud (S3/GCS/FS)
//! ┌──────────┐               ┌──────────────────────┐
//! │ Storage  │──push──►      │ {ns}/deltas/{dev}/    │
//! │ Engine   │               │   1000.delta (sealed) │
//! │          │◄──pull──      │   2000.delta (sealed) │
//! └──────────┘               │ {ns}/manifests/       │
//!                            │   device-a.json       │
//!                            └──────────────────────┘
//! ```
//!
//! Each delta is encrypted with the namespace's `NamespaceKey` —
//! even the cloud provider can't read your data.

pub mod backend;
pub mod conflict;
pub mod delta;
pub mod engine;
pub mod error;
pub mod fs_backend;
pub mod gateway_backend;
pub mod manifest;
pub mod memory_backend;

// Re-export primary types
pub use backend::SyncBackend;
pub use conflict::{ConflictStrategy, SyncConflict};
pub use delta::{DeltaBundle, DeltaMetadata, SealedDelta};
pub use engine::{PullResult, PushResult, SyncEngine};
pub use error::SyncError;
pub use fs_backend::FileSystemSyncBackend;
pub use gateway_backend::{GatewaySyncBackend, Health};
pub use manifest::{SyncManifest, SyncStatus};
pub use memory_backend::MemorySyncBackend;
