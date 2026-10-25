//! Documents exchanged with the bucket: the remote manifest, the local sync
//! index, and the reports the UI renders.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub const MANIFEST_SCHEMA: u32 = 1;
pub const MANIFEST_FILE: &str = "manifest.json";
pub const BLOB_DIR: &str = "blobs";

/// One ledger file, as it exists on one side.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub hash: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub mtime_ms: i64,
}

impl Entry {
    pub fn from_bytes(bytes: &[u8], mtime_ms: i64) -> Self {
        Self {
            hash: blake3::hash(bytes).to_hex().to_string(),
            size: bytes.len() as u64,
            mtime_ms,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub schema: u32,
    pub vault: String,
    pub device: String,
    pub updated_at: String,
    #[serde(default)]
    pub files: BTreeMap<String, Entry>,
}

impl Manifest {
    pub fn empty(vault: &str) -> Self {
        Self {
            schema: MANIFEST_SCHEMA,
            vault: vault.to_string(),
            device: String::new(),
            updated_at: String::new(),
            files: BTreeMap::new(),
        }
    }
}

/// What each file looked like the last time this device finished a sync.
/// `rel -> hash`; a file that is in the ledger but absent here was never seen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalIndex {
    pub schema: u32,
    pub vault: String,
    pub device: String,
    #[serde(default)]
    pub last_sync: Option<String>,
    #[serde(default)]
    pub files: BTreeMap<String, String>,
}

impl LocalIndex {
    pub fn new(vault: &str, device: &str) -> Self {
        Self {
            schema: MANIFEST_SCHEMA,
            vault: vault.to_string(),
            device: device.to_string(),
            last_sync: None,
            files: BTreeMap::new(),
        }
    }
}

/// Everything needed to talk to a bucket. The keys live outside `settings.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncConfig {
    #[serde(default)]
    pub endpoint: String,
    #[serde(default)]
    pub region: String,
    #[serde(default)]
    pub bucket: String,
    #[serde(default)]
    pub prefix: String,
    #[serde(default = "default_path_style")]
    pub path_style: bool,
    #[serde(default)]
    pub access_key: String,
    #[serde(default)]
    pub secret_key: String,
}

fn default_path_style() -> bool {
    true
}

impl SyncConfig {
    /// Trims whitespace and slashes so object keys are stable.
    pub fn normalized(&self) -> Self {
        let mut prefix = self.prefix.trim().trim_matches('/').to_string();
        if prefix.is_empty() {
            prefix = "moneytracker".to_string();
        }
        Self {
            endpoint: self.endpoint.trim().trim_end_matches('/').to_string(),
            region: self.region.trim().to_string(),
            bucket: self.bucket.trim().to_string(),
            prefix,
            path_style: self.path_style,
            access_key: self.access_key.trim().to_string(),
            secret_key: self.secret_key.trim().to_string(),
        }
    }

    pub fn blob_key(&self, hash: &str) -> String {
        format!("{}/{BLOB_DIR}/{hash}", self.normalized().prefix)
    }

    pub fn manifest_key(&self) -> String {
        format!("{}/{MANIFEST_FILE}", self.normalized().prefix)
    }

    /// The bucket is usable when it has an endpoint, a bucket name and keys.
    pub fn is_complete(&self) -> bool {
        !self.endpoint.trim().is_empty()
            && !self.bucket.trim().is_empty()
            && !self.access_key.trim().is_empty()
            && !self.secret_key.trim().is_empty()
    }
}

/// Which side the user keeps for one conflicting file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Local,
    Remote,
}

/// One file both sides changed since the last sync.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conflict {
    pub rel: String,
    /// `None` means the file is missing on that side.
    pub local_hash: Option<String>,
    pub remote_hash: Option<String>,
    pub base_hash: Option<String>,
    pub local_mtime: Option<String>,
    pub remote_mtime: Option<String>,
    pub local_bytes: Option<usize>,
    pub remote_bytes: Option<usize>,
}

/// What a sync did, or why it stopped.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncReport {
    /// Files uploaded (new or changed locally).
    pub pushed: Vec<String>,
    /// Files written locally (new or changed remotely).
    pub pulled: Vec<String>,
    /// Files deleted locally because the remote deleted them.
    pub removed_local: Vec<String>,
    /// Files deleted remotely because this device deleted them.
    pub removed_remote: Vec<String>,
    /// Blocking conflicts; the sync wrote nothing when this is non-empty.
    pub conflicts: Vec<Conflict>,
    pub bytes_up: u64,
    pub bytes_down: u64,
    pub last_sync: Option<String>,
    /// False when the sync stopped early because of conflicts.
    pub complete: bool,
}

impl SyncReport {
    pub fn is_noop(&self) -> bool {
        self.pushed.is_empty()
            && self.pulled.is_empty()
            && self.removed_local.is_empty()
            && self.removed_remote.is_empty()
            && self.conflicts.is_empty()
    }

    pub fn summary(&self) -> String {
        if !self.conflicts.is_empty() {
            return format!("{} 个文件两边都改过，需要你来决定", self.conflicts.len());
        }
        if self.is_noop() {
            return "已经是最新的".to_string();
        }
        let mut parts = Vec::new();
        if !self.pushed.is_empty() {
            parts.push(format!("上传 {}", self.pushed.len()));
        }
        if !self.pulled.is_empty() {
            parts.push(format!("下载 {}", self.pulled.len()));
        }
        if !self.removed_local.is_empty() {
            parts.push(format!("本地删除 {}", self.removed_local.len()));
        }
        if !self.removed_remote.is_empty() {
            parts.push(format!("云端删除 {}", self.removed_remote.len()));
        }
        parts.join(" · ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalises_the_prefix() {
        let config = SyncConfig {
            prefix: " /ledger/ ".to_string(),
            ..Default::default()
        };
        assert_eq!(config.normalized().prefix, "ledger");
        assert_eq!(config.blob_key("abc"), "ledger/blobs/abc");
        assert_eq!(config.manifest_key(), "ledger/manifest.json");
    }

    #[test]
    fn an_empty_prefix_still_has_a_home() {
        assert_eq!(SyncConfig::default().normalized().prefix, "moneytracker");
    }

    #[test]
    fn completeness_needs_keys() {
        let mut config = SyncConfig {
            endpoint: "https://example.com".to_string(),
            bucket: "ledger".to_string(),
            ..Default::default()
        };
        assert!(!config.is_complete());
        config.access_key = "ak".to_string();
        config.secret_key = "sk".to_string();
        assert!(config.is_complete());
    }
}
