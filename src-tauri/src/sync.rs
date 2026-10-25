//! Multi-device sync commands: configuration, a read-only preview, the run
//! itself, and the small DTOs the settings page renders.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager, State};
use tauri_specta::Event;

use ledger_sync::{
    Device, Engine, LocalIndex, Side, SyncConfig, SyncError, SyncReport, INDEX_FILE, SYNC_DIR,
};

use crate::state::{AppState, CmdError, CmdResult};

/// Emitted after a background pull so the UI can reload without polling.
#[derive(Debug, Clone, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct SyncFinished {
    pub summary: String,
    pub complete: bool,
    #[specta(type = f64)]
    pub conflicts: usize,
    #[specta(type = f64)]
    pub pulled: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SyncConfigDto {
    pub enabled: bool,
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub prefix: String,
    pub path_style: bool,
    pub auto: bool,
    pub device: String,
    pub has_keys: bool,
    /// `AKIA…4F2` — enough to recognise the key, not enough to use it.
    pub access_key_hint: String,
    pub configured: bool,
    pub last_sync: Option<String>,
    pub has_ledger: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SyncPatchDto {
    pub enabled: bool,
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub prefix: String,
    pub path_style: bool,
    pub auto: bool,
    pub device: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SyncConflictDto {
    pub rel: String,
    pub local_hash: Option<String>,
    pub remote_hash: Option<String>,
    pub base_hash: Option<String>,
    pub local_mtime: Option<String>,
    pub remote_mtime: Option<String>,
    #[specta(type = Option<f64>)]
    pub local_bytes: Option<usize>,
    #[specta(type = Option<f64>)]
    pub remote_bytes: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SyncPreviewDto {
    pub uploads: Vec<String>,
    pub downloads: Vec<String>,
    pub delete_remote: Vec<String>,
    pub delete_local: Vec<String>,
    pub conflicts: Vec<SyncConflictDto>,
    pub remote_updated_at: Option<String>,
    pub remote_device: Option<String>,
    pub last_sync: Option<String>,
    #[specta(type = f64)]
    pub files_local: usize,
    #[specta(type = f64)]
    pub files_remote: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SyncReportDto {
    pub pushed: Vec<String>,
    pub pulled: Vec<String>,
    pub removed_local: Vec<String>,
    pub removed_remote: Vec<String>,
    pub conflicts: Vec<SyncConflictDto>,
    #[specta(type = f64)]
    pub bytes_up: u64,
    #[specta(type = f64)]
    pub bytes_down: u64,
    pub last_sync: Option<String>,
    pub complete: bool,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SyncChoiceDto {
    pub rel: String,
    /// `local` keeps this device's version, `remote` takes the bucket's.
    pub side: String,
}

// ---------------------------------------------------------------------------
// plumbing
// ---------------------------------------------------------------------------

pub fn config_of(state: &AppState) -> SyncConfig {
    let settings = state.settings();
    let (access_key, secret_key) = state.sync_keys();
    SyncConfig {
        endpoint: settings.sync_endpoint,
        region: settings.sync_region,
        bucket: settings.sync_bucket,
        prefix: settings.sync_prefix,
        path_style: settings.sync_path_style,
        access_key: access_key.unwrap_or_default(),
        secret_key: secret_key.unwrap_or_default(),
    }
    .normalized()
}

fn device_of(state: &AppState) -> Device {
    let settings = state.settings();
    let name = if settings.sync_device.trim().is_empty() {
        default_device_name()
    } else {
        settings.sync_device.trim().to_string()
    };
    Device::new(&name)
}

fn default_device_name() -> String {
    #[cfg(target_os = "macos")]
    {
        "Mac".to_string()
    }
    #[cfg(target_os = "android")]
    {
        "手机".to_string()
    }
    #[cfg(not(any(target_os = "macos", target_os = "android")))]
    {
        "Mac".to_string()
    }
}

fn index_of(state: &AppState) -> Option<LocalIndex> {
    let vault = state.vault().ok()?;
    let path = vault.root.join(SYNC_DIR).join(INDEX_FILE);
    let src = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&src).ok()
}

fn hint(access_key: &Option<String>) -> String {
    match access_key {
        Some(key) if key.len() > 8 => format!("{}…{}", &key[..4], &key[key.len() - 3..]),
        Some(key) => key.clone(),
        None => String::new(),
    }
}

pub fn config_dto(state: &AppState) -> SyncConfigDto {
    let settings = state.settings();
    let (access_key, _) = state.sync_keys();
    let config = config_of(state);
    let configured = config.is_complete();
    let index = index_of(state);
    SyncConfigDto {
        enabled: settings.sync_enabled,
        endpoint: config.endpoint,
        region: config.region,
        bucket: config.bucket,
        prefix: config.prefix,
        path_style: config.path_style,
        auto: settings.sync_auto,
        device: if settings.sync_device.trim().is_empty() {
            default_device_name()
        } else {
            settings.sync_device
        },
        has_keys: access_key.is_some(),
        access_key_hint: hint(&access_key),
        configured,
        last_sync: index.and_then(|index| index.last_sync),
        has_ledger: state.vault().is_ok(),
    }
}

fn map_config(conflict: &ledger_sync::Conflict) -> SyncConflictDto {
    SyncConflictDto {
        rel: conflict.rel.clone(),
        local_hash: conflict.local_hash.clone(),
        remote_hash: conflict.remote_hash.clone(),
        base_hash: conflict.base_hash.clone(),
        local_mtime: conflict.local_mtime.clone(),
        remote_mtime: conflict.remote_mtime.clone(),
        local_bytes: conflict.local_bytes,
        remote_bytes: conflict.remote_bytes,
    }
}

fn map_report(report: &SyncReport) -> SyncReportDto {
    SyncReportDto {
        pushed: report.pushed.clone(),
        pulled: report.pulled.clone(),
        removed_local: report.removed_local.clone(),
        removed_remote: report.removed_remote.clone(),
        conflicts: report.conflicts.iter().map(map_config).collect(),
        bytes_up: report.bytes_up,
        bytes_down: report.bytes_down,
        last_sync: report.last_sync.clone(),
        complete: report.complete,
        summary: report.summary(),
    }
}

pub fn map_error(err: SyncError) -> CmdError {
    let code = match &err {
        SyncError::Config(_) => "sync.config",
        SyncError::Http(403, _) | SyncError::Http(401, _) => "sync.auth",
        SyncError::Http(404, _) => "sync.missing",
        SyncError::Http(_, _) => "sync.http",
        SyncError::Raced(_) => "sync.raced",
        SyncError::VaultMismatch(_) => "sync.vault_mismatch",
        SyncError::Parse(_) => "sync.parse",
        SyncError::Io(_) => "sync.io",
        SyncError::Message(_) => "sync.failed",
    };
    let mut message = err.message();
    if err.is_auth() {
        message.push_str("（检查 Access Key / Secret Key 和 bucket 权限）");
    }
    CmdError::new(code, message)
}

/// Runs one sync on a blocking thread: the vault is read from disk and the
/// bucket is over the network, so neither belongs on the UI thread.
async fn run_blocking(
    state: &AppState,
    app: &AppHandle,
    choices: BTreeMap<String, Side>,
) -> CmdResult<SyncReportDto> {
    let settings = state.settings();
    let config = config_of(state);
    if !settings.sync_enabled {
        return Err(CmdError::new(
            "sync.disabled",
            "同步还没有打开，先到设置里开启",
        ));
    }
    let root = state.vault()?.root.clone();
    let opts = settings.vault_options();
    let device = device_of(state);
    let handle = app.clone();

    let report = tauri::async_runtime::spawn_blocking(move || {
        let store = ledger_sync::S3Store::new(&config).map_err(map_error)?;
        let engine = Engine::new(&root, &opts, device);
        engine.run(&config, &store, &choices).map_err(map_error)
    })
    .await
    .map_err(|e| CmdError::new("sync.join", e.to_string()))??;

    if report.complete {
        let touched: Vec<String> = report
            .pulled
            .iter()
            .chain(report.removed_local.iter())
            .cloned()
            .collect();
        state.reload_from_disk(&state.settings().vault_options());
        if !touched.is_empty() {
            crate::commands::emit_ledger_changed(&handle, touched, "sync");
        }
    }
    Ok(map_report(&report))
}

// ---------------------------------------------------------------------------
// commands
// ---------------------------------------------------------------------------

#[tauri::command]
#[specta::specta]
pub fn sync_config(state: State<'_, AppState>) -> CmdResult<SyncConfigDto> {
    Ok(config_dto(&state))
}

#[tauri::command]
#[specta::specta]
pub fn sync_save(state: State<'_, AppState>, patch: SyncPatchDto) -> CmdResult<SyncConfigDto> {
    let mut settings = state.settings();
    settings.sync_enabled = patch.enabled;
    settings.sync_endpoint = patch.endpoint.trim().to_string();
    settings.sync_region = if patch.region.trim().is_empty() {
        "us-east-1".to_string()
    } else {
        patch.region.trim().to_string()
    };
    settings.sync_bucket = patch.bucket.trim().to_string();
    settings.sync_prefix = if patch.prefix.trim().is_empty() {
        "moneytracker".to_string()
    } else {
        patch.prefix.trim().trim_matches('/').to_string()
    };
    settings.sync_path_style = patch.path_style;
    settings.sync_auto = patch.auto;
    settings.sync_device = patch.device.trim().to_string();
    state.store_settings(&settings)?;
    Ok(config_dto(&state))
}

#[tauri::command]
#[specta::specta]
pub fn sync_set_keys(
    state: State<'_, AppState>,
    access_key: String,
    secret_key: String,
) -> CmdResult<SyncConfigDto> {
    state.set_sync_keys(&access_key, &secret_key)?;
    Ok(config_dto(&state))
}

/// Reads the bucket (the manifest, at most) so 设置 can prove the credentials
/// and the endpoint work before anything is written.
#[tauri::command]
#[specta::specta]
pub async fn sync_test(state: State<'_, AppState>) -> CmdResult<String> {
    let config = config_of(&state);
    if !config.is_complete() {
        return Err(CmdError::new(
            "sync.config",
            "还差 endpoint / bucket / 密钥，先补全再测试",
        ));
    }
    let root = state.vault()?.root.clone();
    let opts = state.settings().vault_options();
    let device = device_of(&state);
    tauri::async_runtime::spawn_blocking(move || {
        let store = ledger_sync::S3Store::new(&config).map_err(map_error)?;
        let engine = Engine::new(&root, &opts, device);
        match engine.inspect(&config, &store).map_err(map_error)? {
            preview if preview.files_remote == 0 => Ok(format!(
                "连接正常，bucket 里还没有这个账本（本机 {} 个文件会在第一次同步时上传）",
                preview.files_local
            )),
            preview => Ok(format!(
                "连接正常：bucket 里已有 {} 个文件，最近一次由 {} 在 {} 写入",
                preview.files_remote,
                preview
                    .remote_device
                    .unwrap_or_else(|| "未知设备".to_string()),
                preview
                    .remote_updated_at
                    .unwrap_or_else(|| "未知时间".to_string()),
            )),
        }
    })
    .await
    .map_err(|e| CmdError::new("sync.join", e.to_string()))?
}

#[tauri::command]
#[specta::specta]
pub async fn sync_preview(state: State<'_, AppState>) -> CmdResult<SyncPreviewDto> {
    let config = config_of(&state);
    let root = state.vault()?.root.clone();
    let opts = state.settings().vault_options();
    let device = device_of(&state);
    tauri::async_runtime::spawn_blocking(move || {
        let store = ledger_sync::S3Store::new(&config).map_err(map_error)?;
        let engine = Engine::new(&root, &opts, device);
        let preview = engine.inspect(&config, &store).map_err(map_error)?;
        Ok(SyncPreviewDto {
            uploads: preview.uploads,
            downloads: preview.downloads,
            delete_remote: preview.delete_remote,
            delete_local: preview.delete_local,
            conflicts: preview.conflicts.iter().map(map_config).collect(),
            remote_updated_at: preview.remote_updated_at,
            remote_device: preview.remote_device,
            last_sync: preview.last_sync,
            files_local: preview.files_local,
            files_remote: preview.files_remote,
        })
    })
    .await
    .map_err(|e| CmdError::new("sync.join", e.to_string()))?
}

#[tauri::command]
#[specta::specta]
pub async fn sync_run(
    app: AppHandle,
    state: State<'_, AppState>,
    choices: Vec<SyncChoiceDto>,
) -> CmdResult<SyncReportDto> {
    let mut sides: BTreeMap<String, Side> = BTreeMap::new();
    for choice in choices {
        let side = match choice.side.as_str() {
            "local" => Side::Local,
            "remote" => Side::Remote,
            other => {
                return Err(CmdError::new(
                    "sync.config",
                    format!("未知的选择 {other}，只能是 local 或 remote"),
                ))
            }
        };
        sides.insert(choice.rel, side);
    }
    run_blocking(&state, &app, sides).await
}

/// Pulls the bucket once, in the background, right after the vault opens.
/// Never blocks startup and never resolves a conflict on its own: the settings
/// page picks those up the next time it is opened.
pub fn auto_pull(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        if !state.settings().sync_enabled {
            return;
        }
        let config = config_of(&state);
        if !config.is_complete() {
            return;
        }
        let Ok(vault) = state.vault() else { return };
        let root = vault.root.clone();
        let opts = state.settings().vault_options();
        let device = device_of(&state);
        let report = tauri::async_runtime::spawn_blocking(move || {
            let store = ledger_sync::S3Store::new(&config)?;
            let engine = Engine::new(&root, &opts, device);
            engine.run(&config, &store, &BTreeMap::new())
        })
        .await;
        let Ok(Ok(report)) = report else {
            eprintln!("moneytracker: 启动同步失败");
            return;
        };
        let touched: Vec<String> = report
            .pulled
            .iter()
            .chain(report.removed_local.iter())
            .cloned()
            .collect();
        if !touched.is_empty() {
            state.reload_from_disk(&state.settings().vault_options());
            crate::commands::emit_ledger_changed(&app, touched, "sync");
        }
        let _ = SyncFinished {
            summary: report.summary(),
            complete: report.complete,
            conflicts: report.conflicts.len(),
            pulled: report.pulled.len(),
        }
        .emit(&app);
    });
}
