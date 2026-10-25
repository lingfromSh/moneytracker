//! Housekeeping that should happen without the user thinking about it: confirm
//! the recurring entries that are due, keep a monthly report on disk, snapshot the
//! vault into git, and warn about budgets that are about to be exceeded.

use std::path::Path;
use std::process::Command;

use chrono::{Datelike, Local};

use ledger_engine::types::{MaintenanceDto, RecurCommitDto, ScheduleCommitDto};
use ledger_engine::writer::atomic_write;
use ledger_engine::{budget_rows, month_report, recur, EngineError, Result, Vault};

/// Run every automation that the settings allow. Never fails the caller for a
/// single step: each outcome is reported back instead.
pub fn run(vault: &Vault, auto_recur: bool, git_auto_commit: bool) -> MaintenanceDto {
    let today = Local::now().date_naive();
    let month = format!("{:04}-{:02}", today.year(), today.month());

    let recur_outcome: Option<RecurCommitDto> = if auto_recur {
        recur::commit_due(vault, 12, &vault.opts).ok()
    } else {
        None
    };

    // Declared schedules (salary, mortgage, subscriptions) are booked only when
    // the same switch is on; the app reminds otherwise.
    let schedule_outcome: Option<ScheduleCommitDto> = if auto_recur {
        ledger_engine::schedule::commit_due(vault, &vault.opts).ok()
    } else {
        None
    };

    let reports = vault.root().join("reports");
    let report_path = reports.join(format!("{month}.md"));
    let report_written = if report_path.exists() {
        None
    } else {
        let markdown = month_report(vault, &month);
        std::fs::create_dir_all(&reports)
            .ok()
            .and_then(|_| atomic_write(&report_path, &markdown).ok())
            .map(|_| format!("reports/{month}.md"))
    };

    let git = if git_auto_commit {
        snapshot(vault.root(), &today.to_string()).ok().flatten()
    } else {
        None
    };

    let mut alerts: Vec<String> = Vec::new();
    for row in budget_rows(vault, &month) {
        if row.budget <= 0.0 {
            continue;
        }
        if row.ratio >= 1.0 {
            alerts.push(format!(
                "{} 已超预算：{} / {}",
                row.label, row.actual_display, row.budget_display
            ));
        } else if row.ratio >= 0.9 {
            alerts.push(format!(
                "{} 已用 {:.0}%：{} / {}",
                row.label,
                row.ratio * 100.0,
                row.actual_display,
                row.budget_display
            ));
        }
    }

    MaintenanceDto {
        at: Local::now().to_rfc3339(),
        month,
        recur: recur_outcome,
        schedule: schedule_outcome,
        report: report_written,
        git,
        alerts,
    }
}

/// Commit the vault to its own git repository, when it has one.
///
/// The repository is the user's, so this stays minimal: stage everything under the
/// vault and commit with a boring message. A clean tree returns `Ok(None)`.
pub fn snapshot(root: &Path, label: &str) -> Result<Option<String>> {
    if !root.join(".git").exists() {
        return Ok(None);
    }
    let add = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["add", "-A"])
        .output()
        .map_err(|err| EngineError::Io(format!("git add failed: {err}")))?;
    if !add.status.success() {
        return Err(EngineError::Io(format!(
            "git add failed: {}",
            String::from_utf8_lossy(&add.stderr).trim()
        )));
    }

    let status = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["status", "--porcelain"])
        .output()
        .map_err(|err| EngineError::Io(format!("git status failed: {err}")))?;
    if status.stdout.is_empty() {
        return Ok(None);
    }

    let commit = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["commit", "-m", &format!("vault: {label}")])
        .output()
        .map_err(|err| EngineError::Io(format!("git commit failed: {err}")))?;
    if !commit.status.success() {
        return Err(EngineError::Io(format!(
            "git commit failed: {}",
            String::from_utf8_lossy(&commit.stderr).trim()
        )));
    }
    let rev = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .filter(|rev| !rev.is_empty());
    Ok(rev)
}
