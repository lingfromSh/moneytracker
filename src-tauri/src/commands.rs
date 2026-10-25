//! IPC surface. Each command is thin: take the state, delegate to
//! `ledger-engine`, map errors to `CmdError`, emit change events.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{Datelike, NaiveDate};
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

pub(crate) fn emit_ledger_changed(app: &AppHandle, paths: Vec<String>, reason: &str) {
    let _ = LedgerChanged {
        paths,
        reason: reason.to_string(),
    }
    .emit(app);
}

// ---------------------------------------------------------------- session

pub(crate) fn open_and_store(
    app: &AppHandle,
    state: &AppState,
    path: &Path,
) -> CmdResult<OpenVaultResultDto> {
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

#[tauri::command(async)]
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

#[tauri::command(async)]
#[specta::specta]
pub fn vault_open(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> CmdResult<OpenVaultResultDto> {
    let root = PathBuf::from(&path);
    open_and_store(&app, &state, &root)
}

/// 手机上 App 只认自己的私有目录（系统文件选择器给的是 `content://` 地址，
/// `std::fs` 打不开），所以前端需要知道「能放在哪」；桌面返回 `None`，由用户自己选。
#[tauri::command]
#[specta::specta]
pub fn vault_default_dir(app: AppHandle) -> Option<String> {
    #[cfg(mobile)]
    {
        crate::mobile_vault_dir(&app).map(|dir| dir.display().to_string())
    }
    #[cfg(not(mobile))]
    {
        let _ = app;
        None
    }
}

#[tauri::command(async)]
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

#[tauri::command(async)]
#[specta::specta]
pub fn vault_info(state: State<'_, AppState>) -> CmdResult<Option<VaultInfoDto>> {
    Ok(state.vault().ok().map(|vault| vault.info_dto()))
}

#[tauri::command(async)]
#[specta::specta]
pub fn ledger_snapshot(state: State<'_, AppState>) -> CmdResult<SnapshotDto> {
    Ok(state.vault()?.snapshot())
}

#[tauri::command(async)]
#[specta::specta]
pub fn ledger_query(state: State<'_, AppState>, req: QueryRequest) -> CmdResult<TxnPageDto> {
    let vault = state.vault()?;
    Ok(report::query_txns(&vault, &req)?)
}

#[tauri::command(async)]
#[specta::specta]
pub fn dashboard(state: State<'_, AppState>, months: Option<u32>) -> CmdResult<DashboardDto> {
    let vault = state.vault()?;
    Ok(report::dashboard(&vault, months.unwrap_or(12) as usize))
}

#[tauri::command(async)]
#[specta::specta]
pub fn reload_vault(app: AppHandle, state: State<'_, AppState>) -> CmdResult<VaultInfoDto> {
    let opts = state.settings().vault_options();
    let vault = state
        .reload_from_disk(&opts)
        .ok_or_else(|| CmdError::new("engine.vault", "could not reload the vault"))?;
    let info = vault.info_dto();
    emit_ledger_changed(
        &app,
        vec![vault.root.to_string_lossy().to_string()],
        "reloaded",
    );
    Ok(info)
}

// ---------------------------------------------------------------- capture

#[tauri::command(async)]
#[specta::specta]
pub fn capture_parse(state: State<'_, AppState>, text: String) -> CmdResult<CaptureParseDto> {
    let vault = state.vault()?;
    let analysis = capture::analyze(&vault, &text);
    Ok(analysis.to_dto())
}

#[tauri::command(async)]
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
    state.reload_from_disk(&opts);
    if let Ok(vault) = state.vault() {
        broadcast_diagnostics(app, vault.diagnostics().len());
    }
    emit_ledger_changed(app, paths, reason);
}

// ---------------------------------------------------------------- transactions

#[tauri::command(async)]
#[specta::specta]
pub fn txn_get(state: State<'_, AppState>, id: String) -> CmdResult<TxnDto> {
    let vault = state.vault()?;
    let idx = vault
        .transaction_index(&id)
        .ok_or_else(|| CmdError::new("app.not_found", format!("transaction {id} not found")))?;
    Ok(vault.txn_dto(idx))
}

#[tauri::command(async)]
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

#[tauri::command(async)]
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

#[tauri::command(async)]
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

#[tauri::command(async)]
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

#[tauri::command(async)]
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

#[tauri::command(async)]
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

#[tauri::command(async)]
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

#[tauri::command(async)]
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

#[tauri::command(async)]
#[specta::specta]
pub fn accounts_tree(state: State<'_, AppState>) -> CmdResult<Vec<AccountNodeDto>> {
    Ok(state.vault()?.account_tree())
}

#[tauri::command(async)]
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

#[tauri::command(async)]
#[specta::specta]
pub fn budget_list(state: State<'_, AppState>) -> CmdResult<Vec<BudgetDto>> {
    Ok(report::budgets(state.vault()?.as_ref()))
}

#[tauri::command(async)]
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

#[tauri::command(async)]
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
#[tauri::command(async)]
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

#[tauri::command(async)]
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

// ---------------------------------------------------------------- import

#[tauri::command(async)]
#[specta::specta]
pub fn import_probe(path: String) -> CmdResult<ImportProbeDto> {
    ledger_engine::import_probe(Path::new(&path)).map_err(CmdError::from)
}

#[tauri::command(async)]
#[specta::specta]
pub fn import_preview(
    state: State<'_, AppState>,
    profile: ImportProfileDto,
) -> CmdResult<ImportPreviewDto> {
    let vault = state.vault()?;
    ledger_engine::import_preview(&vault, &profile).map_err(CmdError::from)
}

#[tauri::command(async)]
#[specta::specta]
pub fn import_commit(
    app: AppHandle,
    state: State<'_, AppState>,
    profile: ImportProfileDto,
) -> CmdResult<ImportCommitDto> {
    let vault = state.vault()?;
    let opts = state.settings().vault_options();
    let audit = state.audit_path();
    let result = ledger_engine::import_commit(&vault, &profile, &opts)?;
    for file in &result.files {
        let _ = writer::append_audit(
            &audit,
            &AuditEntryDto {
                ts: chrono::Local::now().to_rfc3339(),
                op: "import".into(),
                file: file.clone(),
                span_start: 0,
                span_end: 0,
                before: String::new(),
                after: format!("batch {} · {} rows", result.batch_id, result.written),
            },
        );
    }
    refresh(&state, &app, result.files.clone(), "import");
    Ok(result)
}

// ---------------------------------------------------------------- reconcile

#[tauri::command(async)]
#[specta::specta]
pub fn reconcile_preview(
    state: State<'_, AppState>,
    account: String,
    as_of: Option<String>,
) -> CmdResult<ReconcilePreviewDto> {
    let vault = state.vault()?;
    let limit = match as_of.as_deref() {
        Some(raw) if !raw.trim().is_empty() => Some(parse_date_arg(raw)?),
        _ => None,
    };
    Ok(ledger_engine::reconcile_preview(&vault, &account, limit))
}

#[tauri::command(async)]
#[specta::specta]
pub fn reconcile_clear(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Vec<String>,
) -> CmdResult<u32> {
    let opts = state.settings().vault_options();
    let audit = state.audit_path();
    let root = state.vault()?.root().to_path_buf();
    let mut done = 0u32;
    for id in &ids {
        // Re-read between edits: each write moves the spans of the transactions
        // that follow it, so a cached vault would splice the wrong bytes.
        let vault = Vault::load(&root, &opts)?;
        let idx = vault
            .transaction_index(id)
            .ok_or_else(|| CmdError::new("app.not_found", format!("transaction {id} not found")))?;
        if vault.transactions()[idx].txn.status == Some(ledger_core::Status::Cleared) {
            continue;
        }
        let patch = SaveTxnPatch {
            status: Some("*".into()),
            ..Default::default()
        };
        editing::update_txn(&vault, id, &patch, &opts, &audit)?;
        done += 1;
    }
    refresh(&state, &app, Vec::new(), "reconcile");
    Ok(done)
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

/// Releases the global capture shortcut while 设置 records a new chord, then
/// re-registers the stored one. Without this, pressing the current combination
/// opens the capture window and takes focus away mid-recording.
#[tauri::command]
#[specta::specta]
pub fn shortcut_recording(
    app: AppHandle,
    state: State<'_, AppState>,
    active: bool,
) -> CmdResult<()> {
    let settings = state.settings();
    if active {
        crate::shortcut::unregister(&app, &settings.shortcut);
        return Ok(());
    }
    match crate::shortcut::register(&app, &settings.shortcut) {
        Ok(()) => {
            state.set_shortcut_error(None);
            Ok(())
        }
        Err(err) => {
            state.set_shortcut_error(Some(err.clone()));
            Err(CmdError::new("app.shortcut_unavailable", err))
        }
    }
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

#[tauri::command(async)]
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

// ---------------------------------------------------------------- llm

/// What 设置 needs to know about the natural-language client. The API key
/// itself never leaves the backend — only whether one is stored.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct LlmStatusDto {
    pub enabled: bool,
    pub provider: String,
    pub protocol: String,
    pub model: String,
    pub base_url: String,
    pub has_key: bool,
    pub key_required: bool,
    pub ready: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct LlmTestDto {
    pub model: String,
    pub message: String,
    #[specta(type = f64)]
    pub elapsed_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct LlmDraftDto {
    /// The capture line the model's draft was turned into — exactly what Enter commits.
    pub line: String,
    pub payee: String,
    pub date: Option<String>,
    pub category: Option<String>,
    pub funding: Option<String>,
    pub confidence: Option<f64>,
    pub notes: Option<String>,
    pub preview: CaptureParseDto,
    #[specta(type = f64)]
    pub elapsed_ms: f64,
}

fn llm_config(state: &AppState) -> crate::llm::LlmConfig {
    let settings = state.settings();
    crate::llm::LlmConfig::resolve(
        &settings.llm_provider,
        &settings.llm_protocol,
        &settings.llm_base_url,
        &settings.llm_model,
        &state.llm_key().unwrap_or_default(),
    )
}

fn llm_status_of(state: &AppState) -> LlmStatusDto {
    let settings = state.settings();
    let config = llm_config(state);
    let reason = config.block_reason();
    let ready = settings.llm_enabled && config.is_ready();
    LlmStatusDto {
        enabled: settings.llm_enabled,
        provider: config.provider.clone(),
        protocol: config.protocol.as_str().to_string(),
        model: config.model.clone(),
        base_url: config.base_url.clone(),
        has_key: !config.api_key.is_empty(),
        key_required: config.needs_key(),
        ready,
        reason,
    }
}

/// Accounts the model is told about, most used first.
fn account_context(vault: &Vault) -> Vec<String> {
    fn walk(nodes: &[AccountNodeDto], out: &mut Vec<(usize, String)>) {
        for node in nodes {
            out.push((node.txn_count, node.full.clone()));
            walk(&node.children, out);
        }
    }
    let mut all = Vec::new();
    walk(&vault.account_tree(), &mut all);
    all.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    let mut names: Vec<String> = all.into_iter().map(|(_, name)| name).collect();
    // Rules name the categories this ledger intends to use, even before the
    // first transaction touches them — the model should see them too.
    for rule in &vault.rules().rules {
        for posting in &rule.template {
            names.push(posting.account.clone());
        }
        if let Some(from) = &rule.from_account {
            names.push(from.clone());
        }
    }
    names
}

#[tauri::command]
#[specta::specta]
pub fn llm_status(state: State<'_, AppState>) -> CmdResult<LlmStatusDto> {
    Ok(llm_status_of(&state))
}

#[tauri::command]
#[specta::specta]
pub fn llm_set_key(state: State<'_, AppState>, key: String) -> CmdResult<LlmStatusDto> {
    state.set_llm_key(&key)?;
    Ok(llm_status_of(&state))
}

/// Round-trips one tiny prompt so 设置 can prove the endpoint and key work.
#[tauri::command]
#[specta::specta]
pub async fn llm_test(state: State<'_, AppState>) -> CmdResult<LlmTestDto> {
    let config = llm_config(&state);
    if let Some(reason) = config.block_reason() {
        return Err(CmdError::new("llm.not_configured", reason));
    }
    let (system, user) = crate::llm::test_prompt();
    let model = config.model.clone();
    let (reply, elapsed) =
        tauri::async_runtime::spawn_blocking(move || crate::llm::complete(&config, system, user))
            .await
            .map_err(|e| CmdError::new("llm.join", e.to_string()))?
            .map_err(|e| CmdError::new("llm.request_failed", e))?;
    Ok(LlmTestDto {
        model,
        message: first_line(&reply),
        elapsed_ms: elapsed as f64,
    })
}

/// Drafts one entry from free-form text. The model only picks fields; the line
/// is built here and parsed by the normal capture pipeline, so rules, balance
/// checks and the write path are identical to a typed entry.
#[tauri::command]
#[specta::specta]
pub async fn llm_draft(state: State<'_, AppState>, text: String) -> CmdResult<LlmDraftDto> {
    let settings = state.settings();
    if !settings.llm_enabled {
        return Err(CmdError::new(
            "llm.disabled",
            "自然语言记一笔没有启用，先到设置里打开",
        ));
    }
    if text.trim().is_empty() {
        return Err(CmdError::refused("没有可解析的内容"));
    }
    let config = llm_config(&state);
    if let Some(reason) = config.block_reason() {
        return Err(CmdError::new("llm.not_configured", reason));
    }
    let today = chrono::Local::now().date_naive();
    let today = today.to_string();
    let catalog = match state.vault() {
        Ok(vault) => crate::llm::AccountCatalog::from_names(account_context(&vault)),
        Err(_) => crate::llm::AccountCatalog::default(),
    };
    let system = crate::llm::system_prompt(&today, &settings.base_commodity, &catalog);
    let user = crate::llm::user_prompt(&text);
    let (reply, elapsed) =
        tauri::async_runtime::spawn_blocking(move || crate::llm::complete(&config, &system, &user))
            .await
            .map_err(|e| CmdError::new("llm.join", e.to_string()))?
            .map_err(|e| CmdError::new("llm.request_failed", e))?;
    let value =
        crate::llm::extract_json(&reply).map_err(|e| CmdError::new("llm.bad_response", e))?;
    let mut draft =
        crate::llm::draft_from_json(&value).map_err(|e| CmdError::new("llm.bad_response", e))?;
    // The model may invent accounts or pick the wrong payment method: pin the
    // draft to accounts that exist, and let the note decide the wallet.
    crate::llm::ground_draft(&mut draft, &text, &catalog);
    let line = crate::llm::synthesize_line(&draft, &settings.base_commodity, &today);
    let vault = state.vault()?;
    let analysis = capture::analyze(&vault, &line);
    Ok(LlmDraftDto {
        line,
        payee: draft.payee,
        date: draft.date,
        category: draft.category,
        funding: draft.funding,
        confidence: draft.confidence,
        notes: draft.notes,
        preview: analysis.to_dto(),
        elapsed_ms: elapsed as f64,
    })
}

fn first_line(text: &str) -> String {
    let line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let line = line.trim();
    if line.chars().count() > 80 {
        format!("{}…", line.chars().take(80).collect::<String>())
    } else {
        line.to_string()
    }
}

// ---------------------------------------------------------------- planning

fn first_of_month(date: NaiveDate) -> NaiveDate {
    NaiveDate::from_ymd_opt(date.year(), date.month(), 1).unwrap_or(date)
}

fn first_of_next_month(date: NaiveDate) -> NaiveDate {
    let (year, month) = if date.month() == 12 {
        (date.year() + 1, 1)
    } else {
        (date.year(), date.month() + 1)
    };
    NaiveDate::from_ymd_opt(year, month, 1).unwrap_or(date)
}

fn end_of_month(date: NaiveDate) -> NaiveDate {
    first_of_next_month(date).pred_opt().unwrap_or(date)
}

fn today(vault: &Vault) -> NaiveDate {
    vault
        .info_dto()
        .last_date
        .and_then(|d| ledger_core::parser::parse_date_str(&d))
        .unwrap_or_else(|| chrono::Local::now().date_naive())
}

/// Calendar-aligned reporting periods, newest first. The UI only sends the key.
#[tauri::command(async)]
#[specta::specta]
pub fn report_periods(state: State<'_, AppState>) -> CmdResult<Vec<PeriodDto>> {
    let vault = state.vault()?;
    let today = today(&vault);
    let month_start = first_of_month(today);
    let prev_start = first_of_month(month_start.pred_opt().unwrap_or(month_start));
    let quarter_start_month = ((today.month() - 1) / 3) * 3 + 1;
    let quarter_start =
        NaiveDate::from_ymd_opt(today.year(), quarter_start_month, 1).unwrap_or(today);
    let year_start = NaiveDate::from_ymd_opt(today.year(), 1, 1).unwrap_or(today);
    let period = |key: &str, from: NaiveDate, to: NaiveDate, label: &str| PeriodDto {
        key: key.to_string(),
        from: from.to_string(),
        to: to.to_string(),
        label: label.to_string(),
    };
    Ok(vec![
        period("this-month", month_start, end_of_month(today), "本月"),
        period("last-month", prev_start, end_of_month(prev_start), "上月"),
        period("quarter", quarter_start, today, "本季"),
        period("year", year_start, today, "本年"),
        period("ytd", year_start, today, "年初至今"),
        period(
            "all",
            NaiveDate::from_ymd_opt(today.year() - 10, 1, 1).unwrap_or(year_start),
            today,
            "全部",
        ),
    ])
}

#[tauri::command(async)]
#[specta::specta]
pub fn plan_overview(state: State<'_, AppState>) -> CmdResult<PlanOverviewDto> {
    Ok(ledger_engine::plan::overview(state.vault()?.as_ref()))
}

#[tauri::command(async)]
#[specta::specta]
pub fn goals_get(state: State<'_, AppState>) -> CmdResult<String> {
    Ok(ledger_engine::plan::plan_text(state.vault()?.as_ref()))
}

/// Replace `goals.ledger` wholesale — it is small, hand-edited metadata.
#[tauri::command(async)]
#[specta::specta]
pub fn goals_save(
    app: AppHandle,
    state: State<'_, AppState>,
    text: String,
) -> CmdResult<PlanOverviewDto> {
    let vault = state.vault()?;
    let opts = state.settings().vault_options();
    let root = vault.root().to_path_buf();
    let file = ledger_engine::plan::GOALS_FILE;
    let audit = state.audit_path();
    ledger_engine::writer::write_rel(&root, std::path::Path::new(file), &text, None, &opts)?;
    ledger_engine::writer::append_audit(
        &audit,
        &AuditEntryDto {
            ts: chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            op: "goals_save".to_string(),
            file: file.to_string(),
            span_start: 0,
            span_end: text.len(),
            before: String::new(),
            after: text.chars().take(400).collect(),
        },
    )?;
    let fresh = state
        .reload_from_disk(&opts)
        .ok_or_else(|| CmdError::new("engine.vault", "could not reload the vault"))?;
    emit_ledger_changed(&app, vec![file.to_string()], "goals_saved");
    Ok(ledger_engine::plan::overview(fresh.as_ref()))
}

fn parse_date_arg(value: &str) -> CmdResult<NaiveDate> {
    ledger_core::parser::parse_date_str(value.trim())
        .ok_or_else(|| CmdError::new("app.bad_date", format!("`{value}` 不是日期")))
}

#[tauri::command(async)]
#[specta::specta]
pub fn statement_balance(
    state: State<'_, AppState>,
    as_of: Option<String>,
) -> CmdResult<BalanceSheetDto> {
    let vault = state.vault()?;
    let date = match as_of.as_deref().filter(|s| !s.trim().is_empty()) {
        Some(value) => parse_date_arg(value)?,
        None => today(&vault),
    };
    Ok(ledger_engine::balance_sheet(&vault, date))
}

#[tauri::command(async)]
#[specta::specta]
pub fn statement_income(
    state: State<'_, AppState>,
    from: String,
    to: String,
) -> CmdResult<IncomeStatementDto> {
    let vault = state.vault()?;
    Ok(ledger_engine::income_statement(
        &vault,
        parse_date_arg(&from)?,
        parse_date_arg(&to)?,
    ))
}

#[tauri::command(async)]
#[specta::specta]
pub fn statement_bridge(
    state: State<'_, AppState>,
    from: String,
    to: String,
) -> CmdResult<BridgeDto> {
    let vault = state.vault()?;
    Ok(ledger_engine::net_worth_bridge(
        &vault,
        parse_date_arg(&from)?,
        parse_date_arg(&to)?,
    ))
}

#[tauri::command(async)]
#[specta::specta]
pub fn debt_progress(state: State<'_, AppState>) -> CmdResult<Vec<DebtDto>> {
    Ok(ledger_engine::plan::overview(state.vault()?.as_ref()).debts)
}

#[tauri::command(async)]
#[specta::specta]
pub fn allocation(state: State<'_, AppState>) -> CmdResult<AllocationDto> {
    Ok(ledger_engine::plan::overview(state.vault()?.as_ref()).allocation)
}

#[tauri::command(async)]
#[specta::specta]
pub fn report_markdown(state: State<'_, AppState>, month: String) -> CmdResult<String> {
    let vault = state.vault()?;
    Ok(ledger_engine::plan::month_report(&vault, month.trim()))
}

// ------------------------------------------------------------ automation

/// Recurring patterns found in the ledger, with the ones that are due next.
#[tauri::command(async)]
#[specta::specta]
pub fn recur_list(
    state: State<'_, AppState>,
    lookback_months: Option<u32>,
) -> CmdResult<ledger_engine::types::RecurListDto> {
    let vault = state.vault()?;
    let today = chrono::Local::now().date_naive();
    Ok(ledger_engine::recur_list(
        &vault,
        lookback_months.unwrap_or(12) as usize,
        today,
    ))
}

/// Write the next occurrence of the selected patterns.
#[tauri::command(async)]
#[specta::specta]
pub fn recur_commit(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Vec<String>,
) -> CmdResult<ledger_engine::types::RecurCommitDto> {
    let opts = state.settings().vault_options();
    let audit = state.audit_path();
    let vault = state.vault()?;
    let result = ledger_engine::recur::commit(&vault, &ids, &opts)?;
    for file in &result.files {
        let _ = writer::append_audit(
            &audit,
            &AuditEntryDto {
                ts: chrono::Local::now().to_rfc3339(),
                op: "recur_commit".into(),
                file: file.clone(),
                span_start: 0,
                span_end: 0,
                before: String::new(),
                after: format!("{} 笔定期交易", result.written),
            },
        );
    }
    refresh(&state, &app, result.files.clone(), "recur");
    Ok(result)
}

/// Remember "not this month" for one pattern.
#[tauri::command(async)]
#[specta::specta]
pub fn recur_skip(state: State<'_, AppState>, id: String, month: Option<String>) -> CmdResult<()> {
    let vault = state.vault()?;
    let month = month.unwrap_or_else(|| {
        let today = chrono::Local::now().date_naive();
        format!(
            "{:04}-{:02}",
            chrono::Datelike::year(&today),
            chrono::Datelike::month(&today)
        )
    });
    ledger_engine::recur_skip(vault.root(), &id, &month)?;
    Ok(())
}

// ---------------------------------------------------------------- schedules

/// Declared monthly money from `recurring.ledger`, with the calendar of the next
/// `months` months. `auto_commit` mirrors the settings switch: the card only
/// reminds unless the user turned 自动记入 on.
#[tauri::command(async)]
#[specta::specta]
pub fn schedule_overview(
    state: State<'_, AppState>,
    months: Option<u32>,
) -> CmdResult<ScheduleOverviewDto> {
    let vault = state.vault()?;
    let mut overview = ledger_engine::schedule_overview(
        &vault,
        months.unwrap_or(12) as usize,
        chrono::Local::now().date_naive(),
    );
    overview.auto_commit = state.settings().auto_recur;
    Ok(overview)
}

/// Book one occurrence (salary, mortgage, bill) into its day file.
#[tauri::command(async)]
#[specta::specta]
pub fn schedule_commit(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    month: Option<String>,
    tax: Option<f64>,
) -> CmdResult<ScheduleCommitDto> {
    let opts = state.settings().vault_options();
    let audit = state.audit_path();
    let vault = state.vault()?;
    let tax = tax.and_then(rust_decimal::Decimal::from_f64_retain);
    let result = ledger_engine::schedule::commit(&vault, &id, month.as_deref(), tax, &opts)?;
    for file in &result.files {
        let _ = writer::append_audit(
            &audit,
            &AuditEntryDto {
                ts: chrono::Local::now().to_rfc3339(),
                op: "schedule_commit".into(),
                file: file.clone(),
                span_start: 0,
                span_end: 0,
                before: String::new(),
                after: format!(
                    "定期模板 {id} · {}{}",
                    result.months.join(","),
                    match tax {
                        Some(tax) => format!(" · 个税 {tax}"),
                        None => String::new(),
                    }
                ),
            },
        );
    }
    refresh(&state, &app, result.files.clone(), "schedule");
    Ok(result)
}

/// Book every occurrence that is due this month, in one click.
#[tauri::command(async)]
#[specta::specta]
pub fn schedule_commit_due(
    app: AppHandle,
    state: State<'_, AppState>,
) -> CmdResult<ScheduleCommitDto> {
    let opts = state.settings().vault_options();
    let vault = state.vault()?;
    let result = ledger_engine::schedule::commit_due(&vault, &opts)?;
    refresh(&state, &app, result.files.clone(), "schedule");
    Ok(result)
}

/// Remember "not this month" for one template.
#[tauri::command(async)]
#[specta::specta]
pub fn schedule_skip(
    state: State<'_, AppState>,
    id: String,
    month: Option<String>,
) -> CmdResult<()> {
    let vault = state.vault()?;
    ledger_engine::schedule::skip(&vault, &id, month.as_deref())?;
    Ok(())
}

/// Save the whole `recurring.ledger`. Refuses text that would not parse.
#[tauri::command(async)]
#[specta::specta]
pub fn schedule_save(
    app: AppHandle,
    state: State<'_, AppState>,
    text: String,
) -> CmdResult<ScheduleOverviewDto> {
    let opts = state.settings().vault_options();
    let vault = state.vault()?;
    let root = vault.root().to_path_buf();
    ledger_engine::schedule::save(&root, &text, &opts)?;
    let fresh = state
        .reload_from_disk(&opts)
        .ok_or_else(|| CmdError::new("engine.vault", "could not reload the vault"))?;
    emit_ledger_changed(
        &app,
        vec![ledger_engine::SCHEDULE_FILE.to_string()],
        "schedule_saved",
    );
    let mut overview =
        ledger_engine::schedule_overview(fresh.as_ref(), 12, chrono::Local::now().date_naive());
    overview.auto_commit = state.settings().auto_recur;
    Ok(overview)
}

/// Render the 五险二金 skeleton for the modal, without writing anything.
#[tauri::command(async)]
#[specta::specta]
pub fn schedule_salary_preview(
    state: State<'_, AppState>,
    request: SalaryTemplateDto,
) -> CmdResult<String> {
    let vault = state.vault()?;
    let (commodity, precision) = vault.base_commodity();
    Ok(ledger_engine::schedule::render_salary(
        &request, commodity, precision,
    )?)
}

/// Run the automation pass: recurring entries, monthly report, git snapshot, alerts.
#[tauri::command(async)]
#[specta::specta]
pub fn maintenance_run(
    app: AppHandle,
    state: State<'_, AppState>,
) -> CmdResult<ledger_engine::types::MaintenanceDto> {
    let settings = state.settings();
    let vault = state.vault()?;
    let outcome = crate::maintenance::run(&vault, settings.auto_recur, settings.git_auto_commit);
    if outcome
        .recur
        .as_ref()
        .map(|recur| recur.written > 0)
        .unwrap_or(false)
    {
        refresh(&state, &app, Vec::new(), "maintenance");
    }
    Ok(outcome)
}

/// Deterministic insight bullets for the dashboard.
#[tauri::command(async)]
#[specta::specta]
pub async fn insights(state: State<'_, AppState>) -> CmdResult<Vec<InsightDto>> {
    let vault = state.vault()?;
    Ok(ledger_engine::insights(&vault))
}

/// Ask a question in plain language: the model picks query fields, the engine
/// computes the numbers, and only then does the model get to phrase an answer.
// ─── advisor: 只读财务顾问对话 ────────────────────────────────────────────────

#[tauri::command]
#[specta::specta]
pub fn advisor_load(state: State<'_, AppState>) -> CmdResult<crate::advisor::AdvisorHistoryDto> {
    Ok(crate::advisor::load_history(&state.config_dir))
}

#[tauri::command(async)]
#[specta::specta]
pub fn advisor_ask(
    state: State<'_, AppState>,
    question: String,
    history: Vec<crate::advisor::AdvisorTurnDto>,
) -> CmdResult<crate::advisor::AdvisorReplyDto> {
    let question = question.trim().to_string();
    if question.is_empty() {
        return Err(CmdError::refused("先写一个问题"));
    }
    let settings = state.settings();
    if !settings.llm_enabled {
        return Err(CmdError::new(
            "llm.disabled",
            "顾问没有启用，先到设置 → 模型里打开",
        ));
    }
    let config = llm_config(&state);
    if let Some(reason) = config.block_reason() {
        return Err(CmdError::new("llm.not_configured", reason));
    }
    let vault = state.vault()?;

    let reply = crate::advisor::chat(&vault, &config, &history, &question)
        .map_err(|message| CmdError::new("llm.failed", message))?;

    let mut turns = history;
    turns.push(crate::advisor::AdvisorTurnDto {
        role: "user".into(),
        content: question,
        at: crate::advisor::now_stamp(),
    });
    turns.push(crate::advisor::AdvisorTurnDto {
        role: "assistant".into(),
        content: reply.content.clone(),
        at: crate::advisor::now_stamp(),
    });
    let keep = crate::advisor::MAX_TURNS * 2;
    if turns.len() > keep {
        let drop = turns.len() - keep;
        turns.drain(..drop);
    }
    if let Err(message) = crate::advisor::save_history(&state.config_dir, &turns) {
        eprintln!("moneytracker: 无法保存顾问会话: {message}");
    }
    Ok(reply)
}

#[tauri::command]
#[specta::specta]
pub fn advisor_clear(state: State<'_, AppState>) -> CmdResult<()> {
    crate::advisor::clear_history(&state.config_dir)
        .map_err(|message| CmdError::new("app.io", message))
}

/// 问题本身是否就在问「某一天」——只有这种时候才保留单日窗口。
fn asks_about_a_single_day(question: &str) -> bool {
    ["今天", "今日", "昨天", "前天", "当日", "那一天"]
        .iter()
        .any(|word| question.contains(word))
}

#[tauri::command(async)]
#[specta::specta]
pub async fn llm_ask(state: State<'_, AppState>, question: String) -> CmdResult<AskResultDto> {
    let question = question.trim().to_string();
    if question.is_empty() {
        return Err(CmdError::refused("先写一个问题"));
    }
    let settings = state.settings();
    let vault = state.vault()?;
    if vault.transactions().is_empty() {
        return Err(CmdError::refused("账本还没有交易，先记几笔再问"));
    }

    let config = llm_config(&state);
    if !settings.llm_enabled {
        return Err(CmdError::new(
            "llm.disabled",
            "问答没有启用，先到设置里打开",
        ));
    }
    if let Some(reason) = config.block_reason() {
        return Err(CmdError::new("llm.not_configured", reason));
    }

    let catalog = crate::llm::AccountCatalog::from_names(account_context(&vault));
    let brief = crate::advisor::brief(&vault, chrono::Local::now().date_naive());
    let today = chrono::Local::now().date_naive().to_string();
    let system =
        crate::llm::plan_system_prompt(&today, &settings.base_commodity, &catalog, &brief.headline);
    let user = format!("Question: {question}");
    let (reply, _) =
        tauri::async_runtime::spawn_blocking(move || crate::llm::complete(&config, &system, &user))
            .await
            .map_err(|err| CmdError::new("llm.join", err.to_string()))?
            .map_err(|err| CmdError::new("llm.request_failed", err))?;
    let value =
        crate::llm::extract_json(&reply).map_err(|err| CmdError::new("llm.bad_plan", err))?;
    let mut draft =
        crate::llm::plan_from_json(&value).map_err(|err| CmdError::new("llm.bad_plan", err))?;

    // 小模型很爱把「这个月能不能花 5000」翻成「今天 + 某个分类」。窗口只有一天时，
    // 除非问题本身问的就是某一天，否则按默认为近 12 个月 —— 否则结果永远是 0 笔。
    let single_day = draft.date_from.is_some() && draft.date_from == draft.date_to;
    if single_day && !asks_about_a_single_day(&question) {
        let today = chrono::Local::now().date_naive();
        draft.date_from = Some(
            (today - chrono::Months::new(11))
                .with_day(1)
                .unwrap_or(today)
                .format("%Y-%m-%d")
                .to_string(),
        );
        draft.date_to = Some(today.format("%Y-%m-%d").to_string());
    }

    // Validate every field against the real ledger before running anything.
    let mut accounts: Vec<String> = Vec::new();
    for wanted in &draft.accounts {
        let resolved = catalog
            .resolve(wanted)
            .or_else(|| {
                let (name, _) = vault.resolve_account(wanted);
                if name.is_empty() {
                    None
                } else {
                    Some(name)
                }
            })
            .unwrap_or_else(|| wanted.clone());
        if !accounts.contains(&resolved) {
            accounts.push(resolved);
        }
    }
    let clean_date = |raw: &Option<String>| -> Option<String> {
        let text = raw.as_ref()?.trim().to_string();
        let ok = if text.len() == 7 {
            chrono::NaiveDate::parse_from_str(&format!("{text}-01"), "%Y-%m-%d").is_ok()
        } else {
            chrono::NaiveDate::parse_from_str(&text, "%Y-%m-%d").is_ok()
        };
        if ok {
            Some(text)
        } else {
            None
        }
    };

    let plan = ledger_engine::QueryPlan {
        metric: draft.metric.clone(),
        accounts,
        tags: draft.tags.clone(),
        date_from: clean_date(&draft.date_from),
        date_to: clean_date(&draft.date_to),
        group_by: draft.group_by.clone(),
        limit: draft.limit.unwrap_or(12).clamp(1, 50),
        min_amount: draft.min_amount.as_deref().and_then(|raw| {
            let cleaned: String = raw
                .chars()
                .filter(|c| c.is_ascii_digit() || *c == '.' || *c == ',')
                .collect();
            cleaned
                .replace(',', ".")
                .parse::<rust_decimal::Decimal>()
                .ok()
        }),
    };
    let result = ledger_engine::run_plan(&vault, &plan);
    let markdown = ledger_engine::plan_markdown(&question, &result);

    // Phrasing is optional: if this second call fails, the facts still answer it.
    let answer = {
        let config = llm_config(&state);
        let prompt = crate::llm::answer_prompt(&question, &markdown, &brief.full);
        let system = format!(
            "You are a careful bookkeeping assistant. Base commodity: {}. Answer in Chinese, and only with numbers that appear in the prompt.",
            settings.base_commodity
        );
        tauri::async_runtime::spawn_blocking(move || {
            crate::llm::complete(&config, &system, &prompt)
        })
        .await
        .ok()
        .and_then(|outcome| outcome.ok())
        .map(|(text, _)| text.trim().to_string())
        .filter(|text| !text.is_empty())
    };

    Ok(AskResultDto {
        question,
        plan: ledger_engine::insights::plan_dto(&plan, &result),
        result,
        answer,
        error: None,
    })
}
