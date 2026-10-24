//! IPC surface. Each command is thin: take the state, delegate to
//! `ledger-engine`, map errors to `CmdError`, emit change events.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::NaiveDate;
use ledger_engine::types::*;
use ledger_engine::vault::{create_vault, year_path, VaultOptions};
use ledger_engine::{capture, report, writer, Vault};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, State};
use tauri_specta::Event as _;

use crate::editing;
use crate::state::{AppState, CmdError, CmdResult, Settings};

// ---------------------------------------------------------------- events

#[derive(Debug, Clone, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct LedgerChanged {
    pub paths: Vec<String>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct CaptureSaved {
    pub txn_id: String,
    pub file: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct ParseDiagnostics {
    #[specta(type = f64)]
    pub count: usize,
}

fn emit_ledger_changed(app: &AppHandle, paths: Vec<String>, reason: &str) {
    let _ = LedgerChanged {
        paths,
        reason: reason.to_string(),
    }
    .emit(app);
}

// ---------------------------------------------------------------- session

fn open_and_store(app: &AppHandle, state: &AppState, path: &Path) -> CmdResult<OpenVaultResultDto> {
    let mut settings = state.settings();
    let opts = settings.vault_options();
    let vault = Arc::new(Vault::load(path, &opts)?);
    if !settings
        .recent_vaults
        .first()
        .map(|v| std::path::Path::new(v) == path)
        .unwrap_or(false)
    {
        settings.remember(path);
        state.store_settings(&settings)?;
    }
    let info = vault.info_dto();
    let diagnostics = vault.diagnostics_dto();
    let warnings = vault.warnings.clone();
    state.set_vault(Some(vault));
    crate::watcher::watch(app.clone(), path.to_path_buf());
    emit_ledger_changed(
        app,
        vec![path.to_string_lossy().to_string()],
        "vault_opened",
    );
    Ok(OpenVaultResultDto {
        info,
        diagnostics,
        warnings,
    })
}

#[tauri::command]
#[specta::specta]
pub fn vault_create(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    opts: Option<VaultCreateOptions>,
) -> CmdResult<OpenVaultResultDto> {
    let root = PathBuf::from(&path);
    create_vault(
        &root,
        &opts.unwrap_or(VaultCreateOptions {
            base_commodity: None,
            default_funding: None,
            default_unclassified: None,
            template: Some(true),
        }),
    )?;
    open_and_store(&app, &state, &root)
}

#[tauri::command]
#[specta::specta]
pub fn vault_open(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> CmdResult<OpenVaultResultDto> {
    let root = PathBuf::from(&path);
    open_and_store(&app, &state, &root)
}

#[tauri::command]
#[specta::specta]
pub fn vault_close(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    state.set_vault(None);
    crate::watcher::unwatch(&app);
    emit_ledger_changed(&app, Vec::new(), "vault_closed");
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn vault_recent(state: State<'_, AppState>) -> CmdResult<Vec<String>> {
    Ok(state.settings().recent_vaults)
}

#[tauri::command]
#[specta::specta]
pub fn vault_info(state: State<'_, AppState>) -> CmdResult<Option<VaultInfoDto>> {
    Ok(state
        .vault
        .read()
        .expect("vault poisoned")
        .as_ref()
        .map(|v| v.info_dto()))
}

#[tauri::command]
#[specta::specta]
pub fn ledger_snapshot(state: State<'_, AppState>) -> CmdResult<SnapshotDto> {
    Ok(state.vault()?.snapshot())
}

#[tauri::command]
#[specta::specta]
pub fn ledger_query(state: State<'_, AppState>, req: QueryRequest) -> CmdResult<TxnPageDto> {
    let vault = state.vault()?;
    Ok(report::query_txns(&vault, &req)?)
}

#[tauri::command]
#[specta::specta]
pub fn dashboard(state: State<'_, AppState>, months: Option<u32>) -> CmdResult<DashboardDto> {
    let vault = state.vault()?;
    Ok(report::dashboard(&vault, months.unwrap_or(12) as usize))
}

#[tauri::command]
#[specta::specta]
pub fn reload_vault(app: AppHandle, state: State<'_, AppState>) -> CmdResult<VaultInfoDto> {
    let current = state.vault()?;
    let opts = state.settings().vault_options();
    let vault = Arc::new(Vault::load(&current.root, &opts)?);
    let info = vault.info_dto();
    state.set_vault(Some(vault));
    emit_ledger_changed(
        &app,
        vec![current.root.to_string_lossy().to_string()],
        "reloaded",
    );
    Ok(info)
}

// ---------------------------------------------------------------- capture

#[tauri::command]
#[specta::specta]
pub fn capture_parse(state: State<'_, AppState>, text: String) -> CmdResult<CaptureParseDto> {
    let vault = state.vault()?;
    let analysis = capture::analyze(&vault, &text);
    Ok(analysis.to_dto())
}

#[tauri::command]
#[specta::specta]
pub fn capture_commit(
    app: AppHandle,
    state: State<'_, AppState>,
    text: String,
) -> CmdResult<CaptureCommitDto> {
    let vault = state.vault()?;
    let analysis = capture::analyze(&vault, &text);
    let Some(expanded) = analysis.expanded else {
        return Err(CmdError::new(
            "capture.no_transaction",
            if text.trim_start().starts_with('=') {
                "this is a rule line — use rule_upsert"
            } else {
                "nothing to record"
            },
        ));
    };
    let mut errors: Vec<String> = expanded.errors.clone();
    errors.extend(analysis.errors.iter().cloned());
    if !errors.is_empty() {
        return Err(CmdError::new("capture.invalid", errors.join("; ")));
    }
    let opts = state.settings().vault_options();
    let txn_id = capture::new_txn_id();
    let result = editing::commit(&vault, &expanded, &txn_id, &opts, &state.audit_path())?;
    let _ = CaptureSaved {
        txn_id: result.txn_id.clone(),
        file: result.file.clone(),
    }
    .emit(&app);
    refresh(&state, &app, vec![result.file.clone()], "capture");
    Ok(result)
}

fn refresh(state: &AppState, app: &AppHandle, paths: Vec<String>, reason: &str) {
    let opts = state.settings().vault_options();
    if let Some(current) = state.vault.read().expect("vault poisoned").as_ref() {
        if let Ok(vault) = Vault::load(&current.root, &opts) {
            state.set_vault(Some(Arc::new(vault)));
        }
    }
    if let Ok(vault) = state.vault() {
        broadcast_diagnostics(app, vault.diagnostics().len());
    }
    emit_ledger_changed(app, paths, reason);
}

// ---------------------------------------------------------------- transactions

#[tauri::command]
#[specta::specta]
pub fn txn_get(state: State<'_, AppState>, id: String) -> CmdResult<TxnDto> {
    let vault = state.vault()?;
    let idx = vault
        .transaction_index(&id)
        .ok_or_else(|| CmdError::new("app.not_found", format!("transaction {id} not found")))?;
    Ok(vault.txn_dto(idx))
}

#[tauri::command]
#[specta::specta]
pub fn txn_update(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    patch: SaveTxnPatch,
) -> CmdResult<TxnDto> {
    let vault = state.vault()?;
    let opts = state.settings().vault_options();
    editing::update_txn(&vault, &id, &patch, &opts, &state.audit_path())?;
    let rel = vault
        .transaction(&id)
        .map(|t| t.file.clone())
        .unwrap_or_default();
    refresh(&state, &app, vec![rel], "txn_update");
    let vault = state.vault()?;
    let idx = vault
        .transaction_index(&id)
        .ok_or_else(|| CmdError::new("app.not_found", "transaction vanished after update"))?;
    Ok(vault.txn_dto(idx))
}

#[tauri::command]
#[specta::specta]
pub fn txn_delete(app: AppHandle, state: State<'_, AppState>, id: String) -> CmdResult<()> {
    let vault = state.vault()?;
    let opts = state.settings().vault_options();
    let rel = vault
        .transaction(&id)
        .map(|t| t.file.clone())
        .ok_or_else(|| CmdError::new("app.not_found", format!("transaction {id} not found")))?;
    editing::delete_txn(&vault, &id, &opts, &state.audit_path())?;
    refresh(&state, &app, vec![rel], "txn_delete");
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn txn_bulk_update(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Vec<String>,
    patch: BulkPatch,
) -> CmdResult<u32> {
    let vault = state.vault()?;
    let opts = state.settings().vault_options();
    let done = editing::bulk_update(&vault, &ids, &patch, &opts, &state.audit_path())?;
    refresh(&state, &app, Vec::new(), "txn_bulk_update");
    Ok(done as u32)
}

// ---------------------------------------------------------------- rules

fn rule_dto(vault: &Vault, rule: &ledger_engine::RuleDef) -> RuleDto {
    let diagnostics = vault
        .rules
        .diagnostics
        .iter()
        .filter(|d| d.message.contains(&rule.id))
        .map(ledger_engine::diagnostic_dto)
        .collect();
    RuleDto {
        id: rule.id.clone(),
        query: rule.query_raw.clone(),
        enabled: rule.enabled,
        order: rule.order,
        from_account: rule.from_account.clone(),
        postings: rule.postings_dto(),
        raw: rule.raw.clone(),
        span_start: rule.span.start,
        span_end: rule.span.end,
        diagnostics,
    }
}

#[tauri::command]
#[specta::specta]
pub fn rule_list(state: State<'_, AppState>) -> CmdResult<Vec<RuleDto>> {
    let vault = state.vault()?;
    Ok(vault
        .rules
        .rules
        .iter()
        .map(|r| rule_dto(&vault, r))
        .collect())
}

#[tauri::command]
#[specta::specta]
pub fn rule_upsert(
    app: AppHandle,
    state: State<'_, AppState>,
    raw_line: String,
) -> CmdResult<RuleDto> {
    let vault = state.vault()?;
    let analysis = capture::analyze(&vault, &raw_line);
    if analysis.parsed.kind != capture::Kind::Rule {
        return Err(CmdError::refused(
            "not a rule line (expected `= <query> -> <account>`)",
        ));
    }
    let canonical = analysis.canonical;
    if !analysis.errors.is_empty() {
        return Err(CmdError::new("rules.invalid", analysis.errors.join("; ")));
    }
    let opts = state.settings().vault_options();
    let (src, file_state) = writer::read_rel(
        &vault.root,
        std::path::Path::new(ledger_engine::vault::RULES_FILE),
    )?;
    let mut next = src.clone();
    if !next.trim().is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    if !next.trim().is_empty() {
        next.push('\n');
    }
    next.push_str(&canonical);
    next.push('\n');
    writer::snapshot(
        &vault.root,
        std::path::Path::new(ledger_engine::vault::RULES_FILE),
        opts.max_backups,
    )?;
    writer::write_rel(
        &vault.root,
        std::path::Path::new(ledger_engine::vault::RULES_FILE),
        &next,
        Some(&file_state),
        &opts,
    )?;
    refresh(
        &state,
        &app,
        vec![ledger_engine::vault::RULES_FILE.to_string()],
        "rule_upsert",
    );
    let vault = state.vault()?;
    let rule = vault
        .rules
        .rules
        .iter()
        .find(|r| {
            r.query_raw
                == analysis
                    .parsed
                    .rule_line
                    .as_ref()
                    .map(|l| l.query.clone())
                    .unwrap_or_default()
        })
        .or_else(|| vault.rules.rules.last())
        .ok_or_else(|| {
            CmdError::new("rules.missing", "rule was written but could not be re-read")
        })?;
    Ok(rule_dto(&vault, rule))
}

#[tauri::command]
#[specta::specta]
pub fn rule_toggle(app: AppHandle, state: State<'_, AppState>, id: String) -> CmdResult<RuleDto> {
    let vault = state.vault()?;
    let rule = vault
        .rules
        .get(&id)
        .ok_or_else(|| CmdError::new("app.not_found", format!("rule {id} not found")))?
        .clone();
    let mut next_rule = rule.clone();
    next_rule.enabled = !rule.enabled;
    let opts = state.settings().vault_options();
    let path = std::path::Path::new(ledger_engine::vault::RULES_FILE);
    let (src, file_state) = writer::read_rel(&vault.root, path)?;
    let rendered = ledger_engine::rules::render_rule_block(&next_rule);
    let next = if rule.span.end <= src.len() && rule.span.start < rule.span.end {
        format!(
            "{}{}{}",
            &src[..rule.span.start],
            rendered,
            &src[rule.span.end..]
        )
    } else {
        src.clone()
    };
    writer::snapshot(&vault.root, path, opts.max_backups)?;
    writer::write_rel(&vault.root, path, &next, Some(&file_state), &opts)?;
    refresh(
        &state,
        &app,
        vec![ledger_engine::vault::RULES_FILE.to_string()],
        "rule_toggle",
    );
    let vault = state.vault()?;
    let updated = vault
        .rules
        .get(&id)
        .ok_or_else(|| CmdError::new("rules.missing", "rule disappeared"))?;
    Ok(rule_dto(&vault, updated))
}

#[tauri::command]
#[specta::specta]
pub fn rule_test(state: State<'_, AppState>, id: String, n: u32) -> CmdResult<RuleTestDto> {
    let vault = state.vault()?;
    let rule = vault
        .rules
        .get(&id)
        .ok_or_else(|| CmdError::new("app.not_found", format!("rule {id} not found")))?;
    let limit = n.clamp(1, 500);
    let mut matches = Vec::new();
    let mut first: Option<usize> = None;
    for (idx, _) in vault.txns.iter().enumerate() {
        if Some(rule.id.as_str()) == vault.txns[idx].txn.tag("rule") {
            continue;
        }
        let info = vault.txn_info(idx);
        if rule
            .query
            .as_ref()
            .map(|q| q.matches(&info))
            .unwrap_or(false)
        {
            if first.is_none() {
                first = Some(idx);
            }
            if matches.len() < limit as usize {
                matches.push(vault.txn_dto(idx));
            }
        }
    }
    let expanded = first
        .and_then(|idx| editing::expand_rule(&vault, &vault.txns[idx].txn, rule))
        .map(|postings| {
            postings
                .iter()
                .map(|p| ledger_engine::posting_dto(p, &vault.commodities))
                .collect()
        })
        .unwrap_or_default();
    let mut notes = Vec::new();
    if matches.is_empty() {
        notes.push("no historical transaction matches this rule".to_string());
    }
    Ok(RuleTestDto {
        rule_id: id,
        matched_count: matches.len(),
        expanded_for: first.map(|idx| vault.txn_dto(idx)),
        expanded,
        matches,
        notes,
    })
}

#[tauri::command]
#[specta::specta]
pub fn rule_recompute(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Vec<String>,
) -> CmdResult<RuleRecomputeDto> {
    let vault = state.vault()?;
    let opts = state.settings().vault_options();
    let result = editing::recompute(&vault, &ids, &opts, &state.audit_path())?;
    refresh(&state, &app, Vec::new(), "rule_recompute");
    Ok(result)
}

// ---------------------------------------------------------------- accounts

#[tauri::command]
#[specta::specta]
pub fn accounts_tree(state: State<'_, AppState>) -> CmdResult<Vec<AccountNodeDto>> {
    Ok(state.vault()?.account_tree())
}

#[tauri::command]
#[specta::specta]
pub fn account_upsert(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
    kind: String,
) -> CmdResult<()> {
    let vault = state.vault()?;
    let account = ledger_engine::load::normalize_account(&name);
    if account.is_empty() {
        return Err(CmdError::refused("account name is empty"));
    }
    let kind = match kind.trim().to_uppercase().as_str() {
        "A" | "ASSET" => "A",
        "L" | "LIABILITY" => "L",
        "E" | "EQUITY" => "E",
        "R" | "REVENUE" | "INCOME" => "R",
        "X" | "EXPENSE" => "X",
        other => {
            return Err(CmdError::refused(format!(
                "unknown account type \"{other}\""
            )))
        }
    };
    let path = std::path::Path::new(ledger_engine::vault::ACCOUNTS_FILE);
    let (src, file_state) = writer::read_rel(&vault.root, path)?;
    let exists = src.lines().any(|line| {
        line.trim_start()
            .strip_prefix("account ")
            .map(|rest| rest.split_whitespace().next() == Some(account.as_str()))
            .unwrap_or(false)
    });
    if !exists {
        let mut next = src.clone();
        if !next.is_empty() && !next.ends_with('\n') {
            next.push('\n');
        }
        next.push_str(&format!("account {account}  ; type:{kind}\n"));
        let opts = state.settings().vault_options();
        writer::snapshot(&vault.root, path, opts.max_backups)?;
        writer::write_rel(&vault.root, path, &next, Some(&file_state), &opts)?;
    }
    refresh(
        &state,
        &app,
        vec![ledger_engine::vault::ACCOUNTS_FILE.to_string()],
        "account_upsert",
    );
    Ok(())
}

// ---------------------------------------------------------------- budgets & fx

#[tauri::command]
#[specta::specta]
pub fn budget_list(state: State<'_, AppState>) -> CmdResult<Vec<BudgetDto>> {
    Ok(report::budgets(state.vault()?.as_ref()))
}

#[tauri::command]
#[specta::specta]
pub fn forecast(
    state: State<'_, AppState>,
    months: Option<u32>,
) -> CmdResult<Vec<ForecastPointDto>> {
    Ok(report::forecast_points(
        state.vault()?.as_ref(),
        months.unwrap_or(6) as usize,
    ))
}

#[tauri::command]
#[specta::specta]
pub fn fx_missing(state: State<'_, AppState>) -> CmdResult<Vec<FxMissingDto>> {
    Ok(report::fx_missing(state.vault()?.as_ref()))
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct FxRateRow {
    pub date: String,
    pub from: String,
    pub to: String,
    pub rate: String,
}

/// Append externally fetched rates as `P` directives (the frontend performs the
/// HTTP call so the app itself stays offline).
#[tauri::command]
#[specta::specta]
pub fn fx_apply_rates(
    app: AppHandle,
    state: State<'_, AppState>,
    rows: Vec<FxRateRow>,
) -> CmdResult<FxSyncResultDto> {
    let vault = state.vault()?;
    let path = std::path::Path::new(ledger_engine::vault::PRICES_FILE);
    let (src, file_state) = writer::read_rel(&vault.root, path)?;
    let mut next = src.clone();
    let mut written = Vec::new();
    let mut failed = Vec::new();
    for row in &rows {
        let date = match ledger_core::parser::parse_date_str(&row.date) {
            Some(d) => d,
            None => {
                failed.push(format!("{}: bad date", row.date));
                continue;
            }
        };
        let rate: f64 = match row.rate.parse() {
            Ok(v) => v,
            Err(_) => {
                failed.push(format!("{}: bad rate {}", row.date, row.rate));
                continue;
            }
        };
        let line = format!("P {} {} {} {}", date, row.from, rate, row.to);
        if src.lines().any(|l| l.trim() == line.trim()) {
            continue;
        }
        if !next.is_empty() && !next.ends_with('\n') {
            next.push('\n');
        }
        next.push_str(&line);
        next.push('\n');
        written.push(line);
    }
    if !written.is_empty() {
        let opts = state.settings().vault_options();
        writer::snapshot(&vault.root, path, opts.max_backups)?;
        writer::write_rel(&vault.root, path, &next, Some(&file_state), &opts)?;
        refresh(
            &state,
            &app,
            vec![ledger_engine::vault::PRICES_FILE.to_string()],
            "fx_sync",
        );
    }
    Ok(FxSyncResultDto {
        requested: rows.len(),
        written,
        failed,
        skipped_reason: None,
    })
}

#[tauri::command]
#[specta::specta]
pub fn write_audit(state: State<'_, AppState>, limit: u32) -> CmdResult<Vec<AuditEntryDto>> {
    let path = state.audit_path();
    let Ok(src) = std::fs::read_to_string(path) else {
        return Ok(Vec::new());
    };
    let mut rows: Vec<AuditEntryDto> = src
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    rows.reverse();
    rows.truncate(limit.clamp(1, 1000) as usize);
    Ok(rows)
}

// ------------------------------------------------------------- capture window

#[tauri::command]
#[specta::specta]
pub fn capture_show(app: AppHandle) -> CmdResult<()> {
    crate::window::show_capture(&app);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn capture_hide(app: AppHandle) -> CmdResult<()> {
    crate::window::hide_capture(&app);
    Ok(())
}

// ---------------------------------------------------------------- settings

#[tauri::command]
#[specta::specta]
pub fn settings_get(state: State<'_, AppState>) -> CmdResult<Settings> {
    Ok(state.settings())
}

#[tauri::command]
#[specta::specta]
pub fn settings_set(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: Settings,
) -> CmdResult<Settings> {
    let previous = state.settings();
    if previous.shortcut != settings.shortcut {
        // Rebind before persisting: a chord another app owns must not be written
        // to disk, otherwise the next launch would start with no working hotkey.
        if let Err(err) = crate::shortcut::rebind(&app, &previous.shortcut, &settings.shortcut) {
            state.set_shortcut_error(Some(err.clone()));
            let mut kept = settings.clone();
            kept.shortcut = previous.shortcut.clone();
            state.store_settings(&kept)?;
            return Err(CmdError::new("app.shortcut_unavailable", err));
        }
        state.set_shortcut_error(None);
    }
    state.store_settings(&settings)?;
    if previous.vault_path != settings.vault_path
        || previous.base_commodity != settings.base_commodity
        || previous.base_precision != settings.base_precision
    {
        refresh(&state, &app, Vec::new(), "settings");
    }
    Ok(state.settings())
}

/// Registration state of the global capture shortcut, so settings can warn about
/// a chord that was rejected at boot or after a failed rebind.
#[tauri::command]
#[specta::specta]
pub fn shortcut_status(state: State<'_, AppState>) -> CmdResult<ShortcutStatusDto> {
    let settings = state.settings();
    let error = state.shortcut_error();
    Ok(ShortcutStatusDto {
        shortcut: settings.shortcut,
        ok: error.is_none(),
        error,
    })
}

#[tauri::command]
#[specta::specta]
pub fn sample_vault_path(state: State<'_, AppState>) -> CmdResult<String> {
    Ok(state
        .config_dir
        .join("sample-vault")
        .to_string_lossy()
        .to_string())
}

#[tauri::command]
#[specta::specta]
pub fn create_sample_vault(
    app: AppHandle,
    state: State<'_, AppState>,
) -> CmdResult<OpenVaultResultDto> {
    let root = state.config_dir.join("sample-vault");
    create_vault(
        &root,
        &VaultCreateOptions {
            base_commodity: Some("CNY".into()),
            default_funding: Some("assets:cash".into()),
            default_unclassified: Some("expenses:unclassified".into()),
            template: Some(true),
        },
    )?;
    let today = chrono::Local::now().date_naive();
    seed_sample(&root, today);
    open_and_store(&app, &state, &root)
}

fn seed_sample(root: &std::path::Path, today: NaiveDate) {
    let opts = VaultOptions::default();
    let month = today.format("%Y-%m").to_string();
    let dir = root.join(today.format("%Y").to_string());
    let _ = std::fs::create_dir_all(&dir);
    let month_rel = ledger_engine::vault::month_path(today);
    let year_rel = year_path(today);
    let month_name = month_rel.file_name().unwrap().to_string_lossy().to_string();
    if let Ok(year_src) = std::fs::read_to_string(root.join(&year_rel)) {
        if !year_src.contains(&month_name) {
            let mut next = year_src.clone();
            if !next.is_empty() && !next.ends_with('\n') {
                next.push('\n');
            }
            next.push_str(&format!("include {month_name}\n"));
            let _ = std::fs::write(root.join(&year_rel), next);
        }
    }
    let day_rel = ledger_engine::vault::day_path(today);
    let day_name = day_rel.file_name().unwrap().to_string_lossy().to_string();
    if !root.join(&month_rel).exists() {
        let _ = std::fs::write(root.join(&month_rel), format!("include {day_name}\n"));
    }
    let day_file = root.join(&day_rel);
    if day_file.exists() {
        return;
    }
    let sample = format!(
        "2026-{month}-01 * 工资  ; txn:sample-salary :income:\n    assets:cash            12000.00 CNY\n    income:salary         -12000.00 CNY\n\n2026-{month}-03 * 房租  ; txn:sample-rent :housing:\n    expenses:housing:rent   3200.00 CNY\n    assets:cash            -3200.00 CNY\n\n2026-{month}-05 * 星巴克  ; txn:sample-coffee rule:none :coffee:\n    expenses:food:coffee      45.00 CNY\n    assets:cash              -45.00 CNY\n",
        month = month
    );
    let _ = opts;
    let _ = std::fs::write(day_file, sample);
}

#[tauri::command]
#[specta::specta]
pub fn app_version(app: AppHandle) -> CmdResult<String> {
    Ok(app.package_info().version.to_string())
}

/// Emit current parse diagnostics count (used by the capture window badge).
pub fn broadcast_diagnostics(app: &AppHandle, count: usize) {
    let _ = ParseDiagnostics { count }.emit(app);
}
