//! Shared application state: settings persisted in the app config dir, the
//! currently open vault, and the filesystem watcher handle.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

use ledger_engine::vault::VaultOptions;
use ledger_engine::Vault;
use serde::{Deserialize, Serialize};
use specta::Type;

/// Error shape handed to the frontend. Codes are stable, messages are for humans.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct CmdError {
    pub code: String,
    pub message: String,
}

impl CmdError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn refused(message: impl Into<String>) -> Self {
        Self::new("app.refused", message)
    }

    #[allow(dead_code)]
    pub fn unsupported(message: impl Into<String>) -> Self {
        Self::new("app.unsupported", message)
    }
}

impl From<ledger_engine::EngineError> for CmdError {
    fn from(e: ledger_engine::EngineError) -> Self {
        Self::new(e.code(), e.message())
    }
}

pub type CmdResult<T> = std::result::Result<T, CmdError>;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct Settings {
    pub vault_path: Option<String>,
    pub recent_vaults: Vec<String>,
    /// `system` | `light` | `dark`
    pub theme: String,
    /// Global capture hotkey, Tauri accelerator syntax.
    pub shortcut: String,
    pub base_commodity: String,
    pub base_precision: u32,
    pub default_funding: String,
    pub default_unclassified: String,
    pub fx_enabled: bool,
    #[specta(type = f64)]
    pub max_backups: usize,
    /// Persisted dashboard grid layout (JSON blob owned by the frontend).
    pub dashboard_layout: Option<String>,
    /// Optional natural-language capture through a user-supplied LLM.
    pub llm_enabled: bool,
    /// `openai` | `anthropic` | `deepseek` | `ollama` | `custom`
    pub llm_provider: String,
    /// `openai` | `anthropic`; blank means “whatever the provider preset says”.
    pub llm_protocol: String,
    /// Blank means the provider preset's default endpoint.
    pub llm_base_url: String,
    /// Blank means the provider preset's default model.
    pub llm_model: String,
    /// Confirm recurring transactions automatically on launch.
    pub auto_recur: bool,
    /// Commit the vault to its own git repository after maintenance.
    pub git_auto_commit: bool,
    /// `comfortable` | `compact` — dashboard row height and spacing.
    pub density: String,
    /// `1` (Monday) or `0` (Sunday); drives week grouping in lists.
    #[specta(type = f64)]
    pub week_start: u32,
    /// Dashboard cards the user hid, by panel id.
    pub hidden_cards: Vec<String>,
    /// KPI chips to show, by key.
    pub kpi_cards: Vec<String>,
    /// Saved transaction views, a JSON array owned by the frontend:
    /// `[{"name":"餐饮","query":"acct:expenses:food"}]`.
    pub saved_views: Option<String>,
}

/// KPI chips visible until the user picks their own.
pub const DEFAULT_KPI: [&str; 5] = [
    "net_worth",
    "month_income",
    "month_expense",
    "savings_rate",
    "missing_fx",
];

impl Default for Settings {
    fn default() -> Self {
        Self {
            vault_path: None,
            recent_vaults: Vec::new(),
            theme: "system".to_string(),
            shortcut: "CmdOrCtrl+Shift+Space".to_string(),
            base_commodity: "CNY".to_string(),
            base_precision: 2,
            default_funding: "assets:cash".to_string(),
            default_unclassified: "expenses:unclassified".to_string(),
            fx_enabled: false,
            max_backups: 200,
            dashboard_layout: None,
            llm_enabled: false,
            llm_provider: "openai".to_string(),
            llm_protocol: String::new(),
            llm_base_url: String::new(),
            llm_model: String::new(),
            auto_recur: false,
            git_auto_commit: false,
            density: "comfortable".to_string(),
            week_start: 1,
            hidden_cards: Vec::new(),
            kpi_cards: DEFAULT_KPI.iter().map(|key| (*key).to_string()).collect(),
            saved_views: None,
        }
    }
}

impl Settings {
    pub fn vault_options(&self) -> VaultOptions {
        VaultOptions {
            base_commodity: self.base_commodity.clone(),
            base_precision: self.base_precision,
            default_funding: self.default_funding.clone(),
            default_unclassified: self.default_unclassified.clone(),
            fx_enabled: self.fx_enabled,
            max_backups: self.max_backups,
            ..VaultOptions::default()
        }
    }

    pub fn remember(&mut self, path: &Path) {
        let p = path.to_string_lossy().to_string();
        self.vault_path = Some(p.clone());
        self.recent_vaults.retain(|v| v != &p);
        self.recent_vaults.insert(0, p);
        self.recent_vaults.truncate(8);
    }
}

pub struct AppState {
    pub config_dir: PathBuf,
    pub settings: RwLock<Settings>,
    /// Private on purpose: reloads must go through [`AppState::reload_from_disk`],
    /// which never holds a read guard across the store (see its doc comment).
    vault: RwLock<Option<Arc<Vault>>>,
    pub watcher: Mutex<Option<notify::RecommendedWatcher>>,
    /// Last failure while (re)registering the global capture shortcut.
    pub shortcut_error: RwLock<Option<String>>,
    /// Serialises reloads so two commands cannot interleave a load and store.
    reload_lock: Mutex<()>,
}

impl AppState {
    pub fn new(config_dir: PathBuf) -> Self {
        let settings = load_settings(&config_dir).unwrap_or_default();
        Self {
            config_dir,
            settings: RwLock::new(settings),
            vault: RwLock::new(None),
            watcher: Mutex::new(None),
            shortcut_error: RwLock::new(None),
            reload_lock: Mutex::new(()),
        }
    }

    pub fn settings(&self) -> Settings {
        self.settings.read().expect("settings poisoned").clone()
    }

    pub fn store_settings(&self, next: &Settings) -> CmdResult<()> {
        save_settings(&self.config_dir, next)?;
        *self.settings.write().expect("settings poisoned") = next.clone();
        Ok(())
    }

    pub fn vault(&self) -> CmdResult<Arc<Vault>> {
        self.vault
            .read()
            .expect("vault poisoned")
            .clone()
            .ok_or_else(|| CmdError::new("app.no_vault", "no vault is open"))
    }

    pub fn set_vault(&self, vault: Option<Arc<Vault>>) {
        *self.vault.write().expect("vault poisoned") = vault;
    }

    /// Re-reads the vault from disk and swaps it in.
    ///
    /// `std::sync::RwLock` is not re-entrant: taking the write lock while this
    /// thread still holds a read guard deadlocks it, and because commands are
    /// driven from the app's event loop that hangs the whole UI. So the current
    /// handle is cloned into a local, the guard is dropped, and only then does
    /// the (potentially slow) load and the store happen — keep it that way.
    pub fn reload_from_disk(&self, opts: &VaultOptions) -> Option<Arc<Vault>> {
        let _serialised = self.reload_lock.lock().expect("reload poisoned");
        let root = self.vault().ok()?.root.clone();
        let vault = Arc::new(Vault::load(&root, opts).ok()?);
        self.set_vault(Some(vault.clone()));
        Some(vault)
    }

    pub fn shortcut_error(&self) -> Option<String> {
        self.shortcut_error
            .read()
            .expect("shortcut poisoned")
            .clone()
    }

    pub fn set_shortcut_error(&self, error: Option<String>) {
        *self.shortcut_error.write().expect("shortcut poisoned") = error;
    }

    pub fn audit_path(&self) -> PathBuf {
        self.config_dir.join("write-audit.jsonl")
    }

    /// The API key lives outside `settings.json`, in its own user-only file.
    fn llm_secret_path(&self) -> PathBuf {
        self.config_dir.join("llm.json")
    }

    pub fn llm_key(&self) -> Option<String> {
        let src = std::fs::read_to_string(self.llm_secret_path()).ok()?;
        let value: serde_json::Value = serde_json::from_str(&src).ok()?;
        let key = value.get("api_key")?.as_str()?.trim().to_string();
        (!key.is_empty()).then_some(key)
    }

    /// Stores or clears the key. The file is created `0600` so other local
    /// users cannot read it; it never contains anything but the credential.
    pub fn set_llm_key(&self, key: &str) -> CmdResult<()> {
        let path = self.llm_secret_path();
        let key = key.trim();
        if key.is_empty() {
            match std::fs::remove_file(&path) {
                Ok(()) => return Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
                Err(e) => {
                    return Err(CmdError::new(
                        "app.io",
                        format!("无法删除 {}：{e}", path.display()),
                    ))
                }
            }
        }
        std::fs::create_dir_all(&self.config_dir).map_err(|e| {
            CmdError::new(
                "app.io",
                format!("无法创建 {}：{e}", self.config_dir.display()),
            )
        })?;
        let body = serde_json::json!({ "api_key": key }).to_string();
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        use std::io::Write;
        let mut file = options
            .open(&path)
            .map_err(|e| CmdError::new("app.io", format!("无法写入 {}：{e}", path.display())))?;
        file.write_all(body.as_bytes())
            .map_err(|e| CmdError::new("app.io", format!("无法写入 {}：{e}", path.display())))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
        Ok(())
    }
}

/// `~/Library/Application Support/dev.moneytracker.app` on macOS — the same
/// location Tauri's `app_config_dir` resolves to, usable before Tauri boots.
pub fn default_config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("dev.moneytracker.app")
}

fn settings_path(config_dir: &Path) -> PathBuf {
    config_dir.join("settings.json")
}

pub fn load_settings(config_dir: &Path) -> Option<Settings> {
    let src = std::fs::read_to_string(settings_path(config_dir)).ok()?;
    // Forward compatible: unknown fields are ignored, missing ones default.
    serde_json::from_str(&src).ok()
}

pub fn save_settings(config_dir: &Path, settings: &Settings) -> CmdResult<()> {
    std::fs::create_dir_all(config_dir)
        .map_err(|e| CmdError::new("app.io", format!("{}: {e}", config_dir.display())))?;
    let json = serde_json::to_string_pretty(settings)
        .map_err(|e| CmdError::new("app.io", e.to_string()))?;
    std::fs::write(settings_path(config_dir), json)
        .map_err(|e| CmdError::new("app.io", e.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use ledger_engine::capture::{analyze, commit_to_day_file, new_txn_id};
    use ledger_engine::vault::{create_vault, VaultCreateOptions};

    use super::*;

    /// Runs `body` on a worker thread and fails instead of hanging forever: a
    /// `RwLock` deadlock in the write path froze the UI, and a plain call would
    /// simply block the test suite.
    fn with_timeout<F: FnOnce() -> bool + Send + 'static>(name: &str, body: F) {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(body());
        });
        match rx.recv_timeout(Duration::from_secs(20)) {
            Ok(true) => {}
            Ok(false) => panic!("{name}: reload reported failure"),
            Err(_) => panic!("{name}: deadlocked"),
        }
    }

    fn open_state(dir: &std::path::Path, opts: &VaultOptions) -> (AppState, Arc<Vault>) {
        create_vault(dir, &VaultCreateOptions::default()).unwrap();
        let state = AppState::new(dir.to_path_buf());
        let vault = Arc::new(Vault::load(dir, opts).unwrap());
        state.set_vault(Some(vault.clone()));
        (state, vault)
    }

    /// A read guard alive while `set_vault` runs deadlocks `std::sync::RwLock`,
    /// and since commands are driven by the app's event loop that froze the whole
    /// UI the moment a capture was committed.
    #[test]
    fn reloading_does_not_deadlock_while_a_vault_handle_is_alive() {
        let dir = tempfile::tempdir().unwrap();
        let opts = VaultOptions::default();
        let (state, alive) = open_state(dir.path(), &opts);
        let state = Arc::new(state);

        with_timeout("reload", move || state.reload_from_disk(&opts).is_some());
        // The handle stays usable after the swap: readers hold Arcs, not guards.
        assert!(alive.root.exists());
    }

    /// The reported freeze: recording an entry reloaded the vault while a read
    /// guard was still alive, which blocked the main thread forever.
    #[test]
    fn committing_a_capture_and_reloading_keeps_the_index() {
        let dir = tempfile::tempdir().unwrap();
        let opts = VaultOptions::default();
        let (state, vault) = open_state(dir.path(), &opts);
        let state = Arc::new(state);

        let id = new_txn_id();
        let expected = id.clone();
        with_timeout("capture commit", move || {
            let analysis = analyze(&vault, "咖啡 4.5 @微信");
            let expanded = analysis.expanded.expect("capture expands");
            commit_to_day_file(&vault, &expanded, &id, &opts).expect("commit");
            let reloaded = state.reload_from_disk(&opts).expect("reload");
            reloaded.transactions().iter().any(|txn| txn.id == expected)
        });
    }
}

#[cfg(test)]
mod llm_secret_tests {
    use super::*;

    fn state_in(dir: &std::path::Path) -> AppState {
        AppState::new(dir.to_path_buf())
    }

    #[test]
    fn stores_the_api_key_in_its_own_file_and_never_in_settings() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = state_in(dir.path());

        state.set_llm_key("  sk-test-123  ").expect("save key");

        let secret = dir.path().join("llm.json");
        assert!(secret.exists(), "llm.json should exist");
        let body = std::fs::read_to_string(&secret).expect("read");
        assert!(body.contains("sk-test-123"));
        assert!(
            !body.contains("api_key\":\"\""),
            "empty key must clear, not store"
        );

        // The settings document must not carry the credential.
        let settings = dir.path().join("settings.json");
        assert!(
            !settings.exists()
                || !std::fs::read_to_string(&settings)
                    .unwrap()
                    .contains("sk-test"),
            "settings.json must not contain the key"
        );

        assert_eq!(state.llm_key().as_deref(), Some("sk-test-123"));
    }

    #[cfg(unix)]
    #[test]
    fn the_key_file_is_private() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("tempdir");
        let state = state_in(dir.path());
        state.set_llm_key("sk-private").expect("save key");

        let mode = std::fs::metadata(dir.path().join("llm.json"))
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "expected 0600, got {mode:o}");
    }

    #[test]
    fn clearing_the_key_removes_the_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = state_in(dir.path());
        state.set_llm_key("sk-gone").expect("save key");
        assert!(dir.path().join("llm.json").exists());

        state.set_llm_key("   ").expect("clear key");
        assert!(!dir.path().join("llm.json").exists());
        assert!(state.llm_key().is_none());
    }
}
