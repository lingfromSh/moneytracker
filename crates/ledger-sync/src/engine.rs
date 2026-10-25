//! The sync run: scan the vault, diff it against the bucket, move the files.
//!
//! The vault is plain text, and every transaction lives in its own day file, so
//! "sync" is per-file: upload what changed here, write what changed there, and
//! stop the whole run when both sides changed the same file. Nothing is merged
//! and nothing is guessed — the user picks a side and the run is recomputed.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ledger_engine::writer::{file_state, write_rel, FileState};
use ledger_engine::VaultOptions;

use crate::model::{Conflict, Entry, LocalIndex, Manifest, Side, SyncConfig, SyncReport};
use crate::plan::{diff, Plan};
use crate::store::{Condition, ObjectStore};
use crate::SyncError;

pub const SYNC_DIR: &str = ".sync";
pub const INDEX_FILE: &str = "index.json";
pub const TRASH_DIR: &str = "trash";

/// Who is running the sync; recorded in the manifest so the UI can say
/// "last written by your MacBook".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub id: String,
    pub name: String,
}

impl Device {
    pub fn new(name: &str) -> Self {
        let host = std::env::var("COMPUTERNAME")
            .or_else(|_| std::env::var("HOSTNAME"))
            .unwrap_or_default();
        let seed = format!(
            "{}|{}|{}",
            name,
            host,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        Self {
            id: blake3::hash(seed.as_bytes()).to_hex().to_string()[..12].to_string(),
            name: name.trim().to_string(),
        }
    }
}

/// A read-only look at what a sync would do, for the UI.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Preview {
    pub uploads: Vec<String>,
    pub downloads: Vec<String>,
    pub delete_remote: Vec<String>,
    pub delete_local: Vec<String>,
    pub conflicts: Vec<Conflict>,
    pub remote_updated_at: Option<String>,
    pub remote_device: Option<String>,
    pub last_sync: Option<String>,
    pub files_local: usize,
    pub files_remote: usize,
}

impl Preview {
    pub fn plan(&self) -> &[Conflict] {
        &self.conflicts
    }
}

/// Everything one sync run needs to know before it writes anything.
struct PlanContext {
    plan: Plan,
    local: BTreeMap<String, Entry>,
    manifest: Option<Manifest>,
    etag: Option<String>,
    index: LocalIndex,
}

pub struct Engine<'a> {
    root: &'a Path,
    opts: &'a VaultOptions,
    device: Device,
}

impl<'a> Engine<'a> {
    pub fn new(root: &'a Path, opts: &'a VaultOptions, device: Device) -> Self {
        Self { root, opts, device }
    }

    fn sync_dir(&self) -> PathBuf {
        self.root.join(SYNC_DIR)
    }

    fn index_path(&self) -> PathBuf {
        self.sync_dir().join(INDEX_FILE)
    }

    /// Files the app owns: everything ending in `.ledger`, except snapshots and
    /// editor droppings.
    pub fn scan(&self) -> Result<BTreeMap<String, Entry>, SyncError> {
        let mut out = BTreeMap::new();
        walk(self.root, "", &mut out)?;
        Ok(out)
    }

    fn load_index(&self) -> Option<LocalIndex> {
        let src = std::fs::read_to_string(self.index_path()).ok()?;
        serde_json::from_str(&src).ok()
    }

    fn store_index(&self, index: &LocalIndex) -> Result<(), SyncError> {
        std::fs::create_dir_all(self.sync_dir())
            .map_err(|e| SyncError::Io(format!("{}: {e}", self.sync_dir().display())))?;
        let body =
            serde_json::to_string_pretty(index).map_err(|e| SyncError::Parse(e.to_string()))?;
        std::fs::write(self.index_path(), body)
            .map_err(|e| SyncError::Io(format!("{}: {e}", self.index_path().display())))
    }

    /// The vault id: taken from the bucket when the bucket already has one, so
    /// every device adopts the same id; generated on the first push.
    fn vault_id(&self, index: &Option<LocalIndex>, manifest: &Option<Manifest>) -> String {
        if let Some(manifest) = manifest {
            return manifest.vault.clone();
        }
        if let Some(index) = index {
            if !index.vault.is_empty() {
                return index.vault.clone();
            }
        }
        let seed = format!(
            "{}|{}",
            self.root.display(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        blake3::hash(seed.as_bytes()).to_hex().to_string()[..16].to_string()
    }

    fn fetch_manifest(
        &self,
        config: &SyncConfig,
        store: &dyn ObjectStore,
    ) -> Result<(Option<Manifest>, Option<String>), SyncError> {
        let Some(fetched) = store.get(&config.manifest_key())? else {
            return Ok((None, None));
        };
        let manifest: Manifest = serde_json::from_str(&fetched.text()?)
            .map_err(|e| SyncError::Parse(format!("manifest.json 解析失败：{e}")))?;
        Ok((Some(manifest), fetched.etag))
    }

    fn plan_for(
        &self,
        config: &SyncConfig,
        store: &dyn ObjectStore,
        extra_base: &BTreeMap<String, String>,
    ) -> Result<PlanContext, SyncError> {
        let local = self.scan()?;
        let index = self.load_index();
        let (manifest, etag) = self.fetch_manifest(config, store)?;

        if let (Some(index), Some(manifest)) = (&index, &manifest) {
            if !index.vault.is_empty() && index.vault != manifest.vault {
                return Err(SyncError::VaultMismatch(format!(
                    "这个 bucket 属于另一个账本（{}），本地是 {}",
                    manifest.vault, index.vault
                )));
            }
        }

        let mut base: BTreeMap<String, String> = index
            .as_ref()
            .map(|index| index.files.clone())
            .unwrap_or_default();
        base.extend(extra_base.clone());

        let remote = manifest
            .as_ref()
            .map(|manifest| manifest.files.clone())
            .unwrap_or_default();
        let plan = diff(&local, &remote, &base);

        let vault_id = self.vault_id(&index, &manifest);
        let mut adopted = index.unwrap_or_else(|| LocalIndex::new(&vault_id, &self.device.id));
        adopted.vault = vault_id;
        Ok(PlanContext {
            plan,
            local,
            manifest,
            etag,
            index: adopted,
        })
    }

    pub fn inspect(
        &self,
        config: &SyncConfig,
        store: &dyn ObjectStore,
    ) -> Result<Preview, SyncError> {
        let config = config.normalized();
        if !config.is_complete() {
            return Err(SyncError::Config(
                "同步还没配置完整：需要 endpoint、bucket、Access Key、Secret Key".to_string(),
            ));
        }
        let PlanContext {
            plan,
            local,
            manifest,
            index,
            ..
        } = self.plan_for(&config, store, &BTreeMap::new())?;
        Ok(Preview {
            uploads: plan.uploads,
            downloads: plan.downloads,
            delete_remote: plan.delete_remote,
            delete_local: plan.delete_local,
            conflicts: plan.conflicts,
            remote_updated_at: manifest.as_ref().map(|m| m.updated_at.clone()),
            remote_device: manifest.as_ref().map(|m| m.device.clone()),
            last_sync: index.last_sync,
            files_local: local.len(),
            files_remote: manifest.map(|m| m.files.len()).unwrap_or(0),
        })
    }

    /// The full run. Retries once when the manifest moved underneath it.
    pub fn run(
        &self,
        config: &SyncConfig,
        store: &dyn ObjectStore,
        choices: &BTreeMap<String, Side>,
    ) -> Result<SyncReport, SyncError> {
        let config = config.normalized();
        if !config.is_complete() {
            return Err(SyncError::Config(
                "同步还没配置完整：需要 endpoint、bucket、Access Key、Secret Key".to_string(),
            ));
        }
        let mut applied: BTreeMap<String, String> = BTreeMap::new();
        let mut last_error = None;
        for _ in 0..3 {
            match self.run_once(&config, store, choices, &mut applied) {
                Ok(report) => return Ok(report),
                Err(SyncError::Raced(detail)) => {
                    last_error = Some(detail);
                    std::thread::sleep(std::time::Duration::from_millis(120));
                }
                Err(other) => return Err(other),
            }
        }
        Err(SyncError::Raced(last_error.unwrap_or_else(|| {
            "云端 manifest 一直在变，稍后再试".to_string()
        })))
    }

    fn run_once(
        &self,
        config: &SyncConfig,
        store: &dyn ObjectStore,
        choices: &BTreeMap<String, Side>,
        applied: &mut BTreeMap<String, String>,
    ) -> Result<SyncReport, SyncError> {
        let PlanContext {
            mut plan,
            local,
            manifest,
            etag,
            mut index,
        } = self.plan_for(config, store, applied)?;

        if !choices.is_empty() {
            let remote = manifest
                .as_ref()
                .map(|m| m.files.clone())
                .unwrap_or_default();
            plan.resolve(choices, &local, &remote);
        }

        if plan.has_conflicts() {
            return Ok(SyncReport {
                conflicts: plan.conflicts,
                complete: false,
                last_sync: index.last_sync,
                ..Default::default()
            });
        }

        let mut report = SyncReport {
            complete: true,
            ..Default::default()
        };

        // 1. Blobs first: they are content-addressed, so re-uploading the same
        //    bytes is a no-op and concurrent writers cannot corrupt each other.
        for rel in &plan.uploads {
            let path = self.root.join(rel);
            let bytes = std::fs::read(&path).map_err(|e| SyncError::Io(format!("{rel}: {e}")))?;
            let hash = blake3::hash(&bytes).to_hex().to_string();
            let blob = config.blob_key(&hash);
            if !store.head(&blob)? {
                store.put(&blob, &bytes, Condition::Any)?;
                report.bytes_up += bytes.len() as u64;
            }
            report.pushed.push(rel.clone());
        }

        // 2. Then the local files the remote changed.
        for rel in &plan.downloads {
            // A file the user resolved towards "remote" that the manifest does
            // not know about cannot be fetched.
            let Some(remote_entry) = manifest.as_ref().and_then(|m| m.files.get(rel)).cloned()
            else {
                continue;
            };
            let Some(fetched) = store.get(&config.blob_key(&remote_entry.hash))? else {
                return Err(SyncError::Message(format!(
                    "云端缺少 {rel} 的内容（{}），可能被手工删过",
                    remote_entry.hash
                )));
            };
            let text = fetched.text()?;
            let actual = blake3::hash(text.as_bytes()).to_hex().to_string();
            if actual != remote_entry.hash {
                return Err(SyncError::Parse(format!(
                    "{rel} 下载后校验不一致（{} != {}）",
                    actual, remote_entry.hash
                )));
            }
            self.write_file(rel, &text)?;
            applied.insert(rel.clone(), remote_entry.hash.clone());
            report.bytes_down += text.len() as u64;
            report.pulled.push(rel.clone());
        }

        // 3. Deletions: local ones go to `.sync/trash`, remote ones only exist
        //    in the manifest.
        for rel in &plan.delete_local {
            self.trash(rel)?;
            report.removed_local.push(rel.clone());
        }
        for rel in &plan.delete_remote {
            report.removed_remote.push(rel.clone());
        }

        // 4. The new manifest describes exactly what is on disk here now.
        let after = self.scan()?;
        let mut files = manifest
            .as_ref()
            .map(|m| m.files.clone())
            .unwrap_or_default();
        for rel in &plan.uploads {
            if let Some(entry) = after.get(rel) {
                files.insert(rel.clone(), entry.clone());
            }
        }
        for rel in &plan.delete_remote {
            files.remove(rel);
        }
        for rel in &plan.delete_local {
            files.remove(rel);
        }
        // Files this device has never seen but the remote has stay untouched.
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let next = Manifest {
            schema: crate::model::MANIFEST_SCHEMA,
            vault: index.vault.clone(),
            device: self.device.name.clone(),
            updated_at: now.clone(),
            files,
        };
        let body = serde_json::to_vec_pretty(&next).map_err(|e| SyncError::Parse(e.to_string()))?;
        let condition = match &etag {
            Some(etag) => Condition::Match(etag.clone()),
            None => Condition::Create,
        };
        store
            .put(&config.manifest_key(), &body, condition)
            .map_err(|err| match err {
                SyncError::Http(412, _) => {
                    SyncError::Raced("manifest 在同步过程中被别人改过".to_string())
                }
                other => other,
            })?;

        // 5. Remember what we agree on, so the next run can tell who moved.
        index.files = after
            .iter()
            .map(|(rel, entry)| (rel.clone(), entry.hash.clone()))
            .collect();
        index.last_sync = Some(now.clone());
        // Keep the id this vault already had: `Device::new` derives a fresh one
        // every launch, and a changing id would look like a new device each run.
        if index.device.is_empty() {
            index.device = self.device.id.clone();
        }
        index.vault = next.vault.clone();
        self.store_index(&index)?;

        report.last_sync = Some(now);
        Ok(report)
    }

    fn write_file(&self, rel: &str, text: &str) -> Result<(), SyncError> {
        let path = self.root.join(rel);
        let expected: Option<FileState> = if path.is_file() {
            file_state(&path).ok()
        } else {
            None
        };
        write_rel(
            self.root,
            Path::new(rel),
            text,
            expected.as_ref(),
            self.opts,
        )
        .map(|_| ())
        .map_err(|e| SyncError::Message(format!("写入 {rel} 失败：{}", e.message())))
    }

    fn trash(&self, rel: &str) -> Result<(), SyncError> {
        let from = self.root.join(rel);
        if !from.exists() {
            return Ok(());
        }
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let to = self
            .sync_dir()
            .join(TRASH_DIR)
            .join(stamp.to_string())
            .join(rel);
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| SyncError::Io(format!("{}: {e}", parent.display())))?;
        }
        std::fs::rename(&from, &to)
            .map_err(|e| SyncError::Io(format!("{} → {}: {e}", from.display(), to.display())))?;
        Ok(())
    }
}

/// Walks `dir`, collecting every `.ledger` file as `rel -> Entry`.
fn walk(dir: &Path, prefix: &str, out: &mut BTreeMap<String, Entry>) -> Result<(), SyncError> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(SyncError::Io(format!("{}: {err}", dir.display()))),
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name == "node_modules" {
            continue;
        }
        let rel = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        let path = entry.path();
        let meta = match std::fs::metadata(&path) {
            Ok(meta) => meta,
            Err(_) => continue,
        };
        if meta.is_dir() {
            walk(&path, &rel, out)?;
            continue;
        }
        if !name.ends_with(".ledger") || name.ends_with(".tmp") || name.ends_with('~') {
            continue;
        }
        let bytes = std::fs::read(&path).map_err(|e| SyncError::Io(format!("{rel}: {e}")))?;
        let mtime_ms = meta
            .modified()
            .ok()
            .and_then(|at| at.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        out.insert(rel, Entry::from_bytes(&bytes, mtime_ms));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::MemoryStore;
    use ledger_engine::create_vault;
    use ledger_engine::types::VaultCreateOptions;

    fn vault() -> (tempfile::TempDir, VaultOptions) {
        let dir = tempfile::tempdir().unwrap();
        create_vault(dir.path(), &VaultCreateOptions::default()).unwrap();
        (dir, VaultOptions::default())
    }

    fn config() -> SyncConfig {
        SyncConfig {
            endpoint: "https://example.invalid".to_string(),
            bucket: "ledger".to_string(),
            prefix: "vault".to_string(),
            access_key: "ak".to_string(),
            secret_key: "sk".to_string(),
            ..Default::default()
        }
    }

    fn write(path: &Path, body: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, body).unwrap();
    }

    #[test]
    fn the_first_run_pushes_everything() {
        let (dir, opts) = vault();
        let store = MemoryStore::new();
        let engine = Engine::new(dir.path(), &opts, Device::new("test"));
        let report = engine.run(&config(), &store, &BTreeMap::new()).unwrap();
        assert!(report.complete);
        assert_eq!(report.conflicts.len(), 0);
        assert!(report.pushed.contains(&"index.ledger".to_string()));
        assert!(report.pushed.contains(&"accounts.ledger".to_string()));
        let manifest = store.get("vault/manifest.json").unwrap().unwrap();
        let manifest: Manifest = serde_json::from_str(&manifest.text().unwrap()).unwrap();
        assert!(manifest.files.contains_key("index.ledger"));
        assert_eq!(manifest.device, "test");
        assert!(manifest.vault.len() >= 8);
    }

    #[test]
    fn a_second_run_is_a_noop() {
        let (dir, opts) = vault();
        let store = MemoryStore::new();
        let engine = Engine::new(dir.path(), &opts, Device::new("test"));
        engine.run(&config(), &store, &BTreeMap::new()).unwrap();
        let again = engine.run(&config(), &store, &BTreeMap::new()).unwrap();
        assert!(again.is_noop(), "{again:?}");
    }

    #[test]
    fn another_device_receives_the_vault() {
        let (source, opts) = vault();
        let store = MemoryStore::new();
        Engine::new(source.path(), &opts, Device::new("mac"))
            .run(&config(), &store, &BTreeMap::new())
            .unwrap();

        let target = tempfile::tempdir().unwrap();
        let report = Engine::new(target.path(), &opts, Device::new("phone"))
            .run(&config(), &store, &BTreeMap::new())
            .unwrap();
        assert!(report.pulled.contains(&"index.ledger".to_string()));
        assert_eq!(
            std::fs::read_to_string(target.path().join("index.ledger")).unwrap(),
            std::fs::read_to_string(source.path().join("index.ledger")).unwrap()
        );
        // The vault id is adopted from the bucket, not regenerated.
        let index: LocalIndex = serde_json::from_str(
            &std::fs::read_to_string(target.path().join(".sync/index.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(index.vault.len(), 16);
    }

    #[test]
    fn a_new_day_file_travels_and_an_edited_one_conflicts() {
        let (source, opts) = vault();
        let store = MemoryStore::new();
        let mac = Engine::new(source.path(), &opts, Device::new("mac"));
        mac.run(&config(), &store, &BTreeMap::new()).unwrap();

        let phone_dir = tempfile::tempdir().unwrap();
        let phone = Engine::new(phone_dir.path(), &opts, Device::new("phone"));
        phone.run(&config(), &store, &BTreeMap::new()).unwrap();

        // The phone books a transaction on a day the Mac never touched.
        write(
            &phone_dir.path().join("2026/2026-10/2026-10-24.ledger"),
            "2026-10-24 coffee\n    expenses:food    18.00 CNY\n    assets:cash     -18.00 CNY\n",
        );
        let pushed = phone.run(&config(), &store, &BTreeMap::new()).unwrap();
        assert!(pushed
            .pushed
            .contains(&"2026/2026-10/2026-10-24.ledger".to_string()));

        let pulled = mac.run(&config(), &store, &BTreeMap::new()).unwrap();
        assert!(pulled
            .pulled
            .contains(&"2026/2026-10/2026-10-24.ledger".to_string()));
        assert!(source
            .path()
            .join("2026/2026-10/2026-10-24.ledger")
            .exists());

        // Now both edit the same day file: the run stops and asks.
        write(
            &phone_dir.path().join("2026/2026-10/2026-10-24.ledger"),
            "2026-10-24 coffee\n    expenses:food    20.00 CNY\n    assets:cash     -20.00 CNY\n",
        );
        write(
            &source.path().join("2026/2026-10/2026-10-24.ledger"),
            "2026-10-24 lunch\n    expenses:food    30.00 CNY\n    assets:cash     -30.00 CNY\n",
        );
        phone.run(&config(), &store, &BTreeMap::new()).unwrap();
        let blocked = mac.run(&config(), &store, &BTreeMap::new()).unwrap();
        assert!(!blocked.complete);
        assert_eq!(blocked.conflicts.len(), 1);
        assert_eq!(blocked.conflicts[0].rel, "2026/2026-10/2026-10-24.ledger");
        // Nothing was written while it was blocked.
        assert!(
            std::fs::read_to_string(source.path().join("2026/2026-10/2026-10-24.ledger"))
                .unwrap()
                .contains("lunch")
        );

        // Pick the local file: it wins, and the phone receives it.
        let mut choices = BTreeMap::new();
        choices.insert("2026/2026-10/2026-10-24.ledger".to_string(), Side::Local);
        let resolved = mac.run(&config(), &store, &choices).unwrap();
        assert!(resolved.complete, "{resolved:?}");
        assert!(resolved
            .pushed
            .contains(&"2026/2026-10/2026-10-24.ledger".to_string()));

        let phone_after = phone.run(&config(), &store, &BTreeMap::new()).unwrap();
        assert!(phone_after.complete, "{phone_after:?}");
        assert!(
            std::fs::read_to_string(phone_dir.path().join("2026/2026-10/2026-10-24.ledger"))
                .unwrap()
                .contains("lunch")
        );

        // And both sides settle: a further run on either side is a no-op.
        assert!(mac
            .run(&config(), &store, &BTreeMap::new())
            .unwrap()
            .is_noop());
        assert!(phone
            .run(&config(), &store, &BTreeMap::new())
            .unwrap()
            .is_noop());
    }

    #[test]
    fn a_deleted_file_disappears_elsewhere_but_is_kept_in_the_trash() {
        let (source, opts) = vault();
        let store = MemoryStore::new();
        let mac = Engine::new(source.path(), &opts, Device::new("mac"));
        mac.run(&config(), &store, &BTreeMap::new()).unwrap();
        let phone_dir = tempfile::tempdir().unwrap();
        let phone = Engine::new(phone_dir.path(), &opts, Device::new("phone"));
        phone.run(&config(), &store, &BTreeMap::new()).unwrap();

        std::fs::remove_file(source.path().join("budgets.ledger")).unwrap();
        let pushed = mac.run(&config(), &store, &BTreeMap::new()).unwrap();
        assert!(pushed
            .removed_remote
            .contains(&"budgets.ledger".to_string()));

        let pulled = phone.run(&config(), &store, &BTreeMap::new()).unwrap();
        assert!(pulled.removed_local.contains(&"budgets.ledger".to_string()));
        assert!(!phone_dir.path().join("budgets.ledger").exists());
        let trashed = walkdir(phone_dir.path().join(".sync/trash"));
        assert!(trashed.iter().any(|p| p.ends_with("budgets.ledger")));
    }

    fn walkdir(dir: PathBuf) -> Vec<String> {
        let mut out = Vec::new();
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return out;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                out.extend(walkdir(path));
            } else {
                out.push(path.to_string_lossy().to_string());
            }
        }
        out
    }

    #[test]
    fn the_sync_folder_is_never_uploaded() {
        let (dir, opts) = vault();
        let store = MemoryStore::new();
        let engine = Engine::new(dir.path(), &opts, Device::new("test"));
        engine.run(&config(), &store, &BTreeMap::new()).unwrap();
        let manifest = store.get("vault/manifest.json").unwrap().unwrap();
        let manifest: Manifest = serde_json::from_str(&manifest.text().unwrap()).unwrap();
        assert!(manifest
            .files
            .keys()
            .all(|rel| !rel.starts_with(".sync") && rel.ends_with(".ledger")));
        assert!(!manifest.files.keys().any(|rel| rel.contains("/.sync/")));
    }

    #[test]
    fn a_bucket_from_another_vault_is_refused() {
        let (dir, opts) = vault();
        let store = MemoryStore::new();
        let mut config = config();
        config.prefix = "vault".to_string();
        store
            .put(
                &config.manifest_key(),
                br#"{"schema":1,"vault":"someone-else","device":"x","updated_at":"","files":{}}"#,
                Condition::Any,
            )
            .unwrap();
        // Give the local side a different id first.
        let engine = Engine::new(dir.path(), &opts, Device::new("test"));
        engine
            .store_index(&LocalIndex::new("our-vault", "test"))
            .unwrap();
        let err = engine.run(&config, &store, &BTreeMap::new()).unwrap_err();
        assert!(matches!(err, SyncError::VaultMismatch(_)), "{err:?}");
    }

    #[test]
    fn inspect_does_not_write() {
        let (source, opts) = vault();
        let store = MemoryStore::new();
        let mac = Engine::new(source.path(), &opts, Device::new("mac"));
        mac.run(&config(), &store, &BTreeMap::new()).unwrap();

        let phone_dir = tempfile::tempdir().unwrap();
        let phone = Engine::new(phone_dir.path(), &opts, Device::new("phone"));
        let preview = phone.inspect(&config(), &store).unwrap();
        assert!(!preview.uploads.is_empty() || !preview.downloads.is_empty());
        assert!(preview.files_remote > 0);
        assert!(!phone_dir.path().join("index.ledger").exists());
        assert!(preview.remote_device.as_deref() == Some("mac"));
    }
}
