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
}

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
    pub vault: RwLock<Option<Arc<Vault>>>,
    pub watcher: Mutex<Option<notify::RecommendedWatcher>>,
}

impl AppState {
    pub fn new(config_dir: PathBuf) -> Self {
        let settings = load_settings(&config_dir).unwrap_or_default();
        Self {
            config_dir,
            settings: RwLock::new(settings),
            vault: RwLock::new(None),
            watcher: Mutex::new(None),
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

    pub fn audit_path(&self) -> PathBuf {
        self.config_dir.join("write-audit.jsonl")
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
