//! Multi-device sync for a MoneyTracker vault over any S3-compatible bucket.
//!
//! The vault is plain text and every transaction lives in its own day file, so
//! syncing it is per-file work: upload what changed here, download what changed
//! there, and stop the run when both sides touched the same file. Nothing is
//! merged automatically — the user picks a side and the run is recomputed.
//!
//! Layout inside the bucket (`prefix` defaults to `moneytracker`):
//!
//! ```text
//! <prefix>/manifest.json      one small JSON document, updated with If-Match
//! <prefix>/blobs/<blake3>     immutable file contents, shared by both sides
//! ```
//!
//! Blobs are content-addressed and never rewritten, which makes concurrent
//! uploaders harmless: two devices pushing the same content push the same
//! object. Only `manifest.json` is a single point of contention, and that is
//! protected by the storage layer's conditional write.

pub mod engine;
pub mod model;
pub mod plan;
pub mod sigv4;
pub mod store;

pub use engine::{Device, Engine, Preview, INDEX_FILE, SYNC_DIR, TRASH_DIR};
pub use model::{
    Conflict, Entry, LocalIndex, Manifest, Side, SyncConfig, SyncReport, MANIFEST_FILE,
    MANIFEST_SCHEMA,
};
pub use plan::{diff, Action, Plan};
pub use store::{Condition, Fetched, MemoryStore, ObjectStore, S3Store};

use thiserror::Error;

#[derive(Debug, Clone, Error)]
pub enum SyncError {
    #[error("{0}")]
    Config(String),
    #[error("{0}")]
    Io(String),
    #[error("{0}")]
    Parse(String),
    #[error("服务端返回 {0}：{1}")]
    Http(u16, String),
    /// The manifest changed between read and write; the caller retries.
    #[error("{0}")]
    Raced(String),
    #[error("{0}")]
    VaultMismatch(String),
    #[error("{0}")]
    Message(String),
}

impl SyncError {
    /// Errors worth showing verbatim to a person.
    pub fn message(&self) -> String {
        self.to_string()
    }

    pub fn is_auth(&self) -> bool {
        matches!(self, SyncError::Http(403, _))
    }
}
