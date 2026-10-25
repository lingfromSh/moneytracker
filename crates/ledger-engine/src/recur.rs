//! Recurring transactions: find the ones that keep coming back, and let the user
//! confirm next month's occurrence in one click.
//!
//! Detection is deliberately conservative — a pattern needs at least three
//! occurrences in three different months with a stable payee, amount and set of
//! postings. Nothing is ever written without an explicit confirmation.

use std::collections::BTreeSet;
use std::path::Path;

use chrono::{Datelike, NaiveDate};
use rust_decimal::Decimal;

use ledger_core::ast::{Posting, Transaction};

use crate::amounts::decimal_to_dto;
use crate::capture::{self, CommittedBlock, ExpandedCapture};
use crate::load::Vault;
use crate::rules::Direction;
use crate::types::{AmountDto, RecurCommitDto, RecurGroupDto, RecurListDto};
use crate::vault::VaultOptions;
use crate::Result;

/// Where "not this month" decisions are kept. Plain text, and deliberately not
/// included by `index.ledger`, exactly like `rules.ledger`.
pub const SKIP_FILE: &str = "recur.ledger";

/// How far ahead an upcoming occurrence still counts as "due".
const DUE_WINDOW_DAYS: i64 = 10;

const HEADER: &str = "; MoneyTracker recurring-transaction decisions — read by the app, never included by index.ledger.\n";

#[derive(Debug, Clone)]
struct Occurrence {
    date: NaiveDate,
    month: String,
    payee: String,
    postings: Vec<(String, Decimal, String)>,
    total: Decimal,
}

#[derive(Debug, Clone)]
pub struct RecurGroup {
    pub id: String,
    pub payee: String,
    pub direction: String,
    pub amount: AmountDto,
    pub day: u32,
    pub occurrences: usize,
    pub months: Vec<String>,
    pub last_date: NaiveDate,
    pub next_date: NaiveDate,
    pub gap_days: i64,
    pub confidence: f64,
    /// Postings of the most recent occurrence, ready to be replayed.
    postings: Vec<(String, Decimal, String)>,
}

fn month_key(date: NaiveDate) -> String {
    format!("{:04}-{:02}", date.year(), date.month())
}

fn add_months(date: NaiveDate, months: i32) -> NaiveDate {
    let mut year = date.year();
    let mut month = date.month() as i32 + months;
    while month > 12 {
        month -= 12;
        year += 1;
    }
    while month < 1 {
        month += 12;
        year -= 1;
    }
    let day = date.day().min(days_in_month(year, month as u32));
    NaiveDate::from_ymd_opt(year, month as u32, day).unwrap_or(date)
}

fn days_in_month(year: i32, month: u32) -> u32 {
    let (next_year, next_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    let first = NaiveDate::from_ymd_opt(year, month, 1).expect("valid month");
    let next = NaiveDate::from_ymd_opt(next_year, next_month, 1).expect("valid month");
    (next - first).num_days() as u32
}

fn median(mut values: Vec<i64>) -> i64 {
    values.sort_unstable();
    if values.is_empty() {
        return 30;
    }
    values[values.len() / 2]
}

fn normalise_payee(payee: &str) -> String {
    payee
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn group_id(key: &str) -> String {
    format!(
        "c-{}",
        &blake3::hash(key.as_bytes()).to_hex().to_string()[..8]
    )
}

/// Read the skip decisions written by [`skip`].
pub fn skip_list(root: &Path) -> BTreeSet<(String, String)> {
    let mut out = BTreeSet::new();
    let file = root.join(SKIP_FILE);
    let Ok(src) = std::fs::read_to_string(file) else {
        return out;
    };
    for line in src.lines() {
        let line = line.trim();
        let body = line.trim_start_matches(';').trim();
        let Some(rest) = body.strip_prefix("skip:") else {
            continue;
        };
        if let Some((id, month)) = rest.split_once('@') {
            out.insert((id.trim().to_string(), month.trim().to_string()));
        }
    }
    out
}

/// Record "not this month" for one group, idempotently.
pub fn skip(root: &Path, id: &str, month: &str) -> Result<()> {
    let file = root.join(SKIP_FILE);
    let mut src = std::fs::read_to_string(&file).unwrap_or_else(|_| HEADER.to_string());
    if !src.ends_with('\n') {
        src.push('\n');
    }
    let line = format!("skip:{id}@{month}");
    if src
        .lines()
        .any(|existing| existing.trim().trim_start_matches(';').trim() == line)
    {
        return Ok(());
    }
    src.push_str(&format!("; {line}\n"));
    crate::writer::atomic_write(&file, &src)
}

/// Find the patterns in `vault` and say which ones are due next month.
pub fn list(vault: &Vault, lookback_months: usize, today: NaiveDate) -> RecurListDto {
    let base = vault.opts.base_commodity.clone();
    let precision = vault.opts.base_precision;
    let earliest = add_months(today, -(lookback_months.min(60) as i32));

    let mut buckets: Vec<(String, Vec<Occurrence>)> = Vec::new();
    for record in vault.transactions() {
        let txn = &record.txn;
        if txn.date < earliest || txn.date > today {
            continue;
        }
        let mut postings: Vec<(String, Decimal, String)> = Vec::new();
        let mut accounts: Vec<String> = Vec::new();
        for value in &record.values {
            postings.push((
                value.account.clone(),
                value.amount.quantity,
                value.amount.commodity.clone(),
            ));
            accounts.push(value.account.clone());
        }
        if postings.is_empty() {
            continue;
        }
        accounts.sort();
        let total = record
            .base_value
            .unwrap_or(Decimal::ZERO)
            .round_dp(precision);
        let key = format!(
            "{}|{}|{}",
            normalise_payee(&txn.payee),
            total,
            accounts.join(",")
        );
        let occurrence = Occurrence {
            date: txn.date,
            month: month_key(txn.date),
            payee: txn.payee.clone(),
            postings,
            total,
        };
        match buckets.iter_mut().find(|(existing, _)| existing == &key) {
            Some((_, rows)) => rows.push(occurrence),
            None => buckets.push((key, vec![occurrence])),
        }
    }

    let skipped = skip_list(&vault.root);
    let mut groups: Vec<RecurGroupDto> = Vec::new();
    let mut raw: Vec<RecurGroup> = Vec::new();

    for (key, mut rows) in buckets {
        rows.sort_by_key(|row| row.date);
        let months: Vec<String> = {
            let mut set: Vec<String> = rows.iter().map(|row| row.month.clone()).collect();
            set.sort();
            set.dedup();
            set
        };
        if rows.len() < 3 || months.len() < 3 {
            continue;
        }
        let gaps: Vec<i64> = rows
            .windows(2)
            .map(|pair| (pair[1].date - pair[0].date).num_days())
            .collect();
        let gap = median(gaps.clone());
        if !(20..=45).contains(&gap) {
            continue;
        }
        let last = rows.last().expect("non-empty").clone();
        let mut days: Vec<i64> = rows.iter().map(|row| row.date.day() as i64).collect();
        days.sort_unstable();
        let day = days[days.len() / 2] as u32;
        let next_month = if last.date.day() >= day {
            add_months(
                NaiveDate::from_ymd_opt(last.date.year(), last.date.month(), day)
                    .unwrap_or(last.date),
                1,
            )
        } else {
            NaiveDate::from_ymd_opt(last.date.year(), last.date.month(), day).unwrap_or(last.date)
        };
        let next_date = next_month;
        let id = group_id(&key);
        let expected_months = ((today.year() - earliest.year()) * 12 + today.month() as i32
            - earliest.month() as i32
            + 1)
        .max(1);
        let confidence = (rows.len() as f64 / expected_months as f64).clamp(0.0, 1.0);
        let direction = if last.total < Decimal::ZERO {
            "expense"
        } else {
            "income"
        };
        let next_month_key = month_key(next_date);
        let already = rows.iter().any(|row| row.month == next_month_key)
            || skipped.contains(&(id.clone(), next_month_key.clone()));
        raw.push(RecurGroup {
            id: id.clone(),
            payee: last.payee.clone(),
            direction: direction.to_string(),
            amount: decimal_to_dto(last.total, &base, precision),
            day,
            occurrences: rows.len(),
            months: months.clone(),
            last_date: last.date,
            next_date,
            gap_days: gap,
            confidence,
            postings: last.postings.clone(),
        });
        groups.push(RecurGroupDto {
            id,
            payee: last.payee,
            direction: direction.to_string(),
            amount: decimal_to_dto(last.total, &base, precision),
            day,
            occurrences: rows.len(),
            months,
            last_date: last.date.to_string(),
            next_date: next_date.to_string(),
            gap_days: gap,
            confidence,
            due: !already && (next_date - today).num_days() <= DUE_WINDOW_DAYS,
            done_reason: if already {
                Some(
                    if skipped.contains(&(group_id(&key), next_month_key.clone())) {
                        "已忽略".to_string()
                    } else {
                        format!("{} 已记录", next_month_key)
                    },
                )
            } else {
                None
            },
        });
    }

    groups.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let due_count = groups.iter().filter(|group| group.due).count();
    REGISTRY.with(|registry| {
        let mut registry = registry.borrow_mut();
        registry.clear();
        for group in raw {
            registry.insert(group.id.clone(), group);
        }
    });
    RecurListDto { groups, due_count }
}

thread_local! {
    /// Groups produced by the last [`list`] call, so [`commit`] can replay them.
    /// The command layer lists first and commits within the same request cycle.
    static REGISTRY: std::cell::RefCell<std::collections::HashMap<String, RecurGroup>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

fn lookup(vault: &Vault, id: &str) -> Option<RecurGroup> {
    let cached = REGISTRY.with(|registry| registry.borrow().get(id).cloned());
    if cached.is_some() {
        return cached;
    }
    // A fresh process has no cache: rebuild it from the ledger.
    let today = chrono::Local::now().date_naive();
    let _ = list(vault, 24, today);
    REGISTRY.with(|registry| registry.borrow().get(id).cloned())
}

/// Write one transaction per requested group, dated at its next occurrence.
pub fn commit(vault: &Vault, ids: &[String], opts: &VaultOptions) -> Result<RecurCommitDto> {
    let mut written = 0usize;
    let mut files: Vec<String> = Vec::new();
    let mut groups: Vec<RecurGroupDto> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    for id in ids {
        let Some(group) = lookup(vault, id) else {
            errors.push(format!("{id}: 找不到这条定期交易"));
            continue;
        };
        let mut txn = Transaction::new(group.next_date, group.payee.clone());
        txn.status = Some(ledger_core::Status::Cleared);
        txn.postings = group
            .postings
            .iter()
            .map(|(account, quantity, commodity)| {
                Posting::new(
                    account.clone(),
                    Some(ledger_core::Amount::canonical(
                        *quantity,
                        commodity,
                        vault.commodities.precision_of(commodity),
                    )),
                )
            })
            .collect();
        txn.tags.push(ledger_core::Tag::kv(
            "recur",
            format!("{}@{}", group.id, month_key(group.next_date)),
        ));
        let direction = if group.direction == "income" {
            Direction::Income
        } else {
            Direction::Expense
        };
        let expanded = ExpandedCapture {
            txn,
            postings: Vec::new(),
            canonical: String::new(),
            warnings: Vec::new(),
            errors: Vec::new(),
            rule_id: Some(group.id.clone()),
            direction,
            summary: group.payee.clone(),
        };
        let txn_id = capture::new_txn_id();
        match capture::commit_to_day_file(vault, &expanded, &txn_id, opts) {
            Ok(CommittedBlock { rel, .. }) => {
                written += 1;
                if !files.contains(&rel) {
                    files.push(rel);
                }
                groups.push(RecurGroupDto {
                    id: group.id.clone(),
                    payee: group.payee.clone(),
                    direction: group.direction.clone(),
                    amount: group.amount.clone(),
                    day: group.day,
                    occurrences: group.occurrences,
                    months: vec![month_key(group.next_date)],
                    last_date: group.last_date.to_string(),
                    next_date: group.next_date.to_string(),
                    gap_days: group.gap_days,
                    confidence: group.confidence,
                    due: false,
                    done_reason: Some(format!("{} 已记录", month_key(group.next_date))),
                });
            }
            Err(err) => errors.push(format!("{}: {}", group.payee, err.message())),
        }
    }

    Ok(RecurCommitDto {
        written,
        skipped: ids.len().saturating_sub(written),
        files,
        groups,
        errors,
    })
}

/// Confirm everything that is due — used by the maintenance job.
pub fn commit_due(
    vault: &Vault,
    lookback_months: usize,
    opts: &VaultOptions,
) -> Result<RecurCommitDto> {
    let today = chrono::Local::now().date_naive();
    let listed = list(vault, lookback_months, today);
    let ids: Vec<String> = listed
        .groups
        .iter()
        .filter(|group| group.due)
        .map(|group| group.id.clone())
        .collect();
    if ids.is_empty() {
        return Ok(RecurCommitDto {
            written: 0,
            skipped: 0,
            files: Vec::new(),
            groups: Vec::new(),
            errors: Vec::new(),
        });
    }
    commit(vault, &ids, opts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::VaultCreateOptions;
    use crate::vault::create_vault;

    fn vault_with(months: &[(&str, &str)]) -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().expect("tempdir");
        let opts = VaultOptions::default();
        create_vault(dir.path(), &VaultCreateOptions::default()).expect("vault");
        for (date, block) in months {
            let day: NaiveDate = date.parse().expect("date");
            let file = crate::writer::ensure_day_file(dir.path(), day, &opts).expect("day file");
            let (src, state) = crate::writer::read_rel(dir.path(), &file.rel).expect("read");
            let mut next = src.clone();
            if !next.is_empty() && !next.ends_with('\n') {
                next.push('\n');
            }
            next.push_str(block);
            next.push('\n');
            crate::writer::write_rel(dir.path(), &file.rel, &next, Some(&state), &opts)
                .expect("write");
        }
        let vault = Vault::load(dir.path(), &opts).expect("load");
        (dir, vault)
    }

    fn salary(date: &str) -> String {
        format!("{date} * 工资\n    assets:bank:cmb         12000.00 CNY\n    income:salary         -12000.00 CNY")
    }

    #[test]
    fn detects_monthly_patterns_and_ignores_one_offs() {
        let (_dir, vault) = vault_with(&[
            ("2026-07-05", &salary("2026-07-05")),
            ("2026-08-05", &salary("2026-08-05")),
            ("2026-09-05", &salary("2026-09-05")),
            (
                "2026-09-20",
                "2026-09-20 * 医院\n    expenses:health   300.00 CNY\n    assets:bank:cmb  -300.00 CNY",
            ),
        ]);
        let today: NaiveDate = "2026-09-25".parse().unwrap();
        let listed = list(&vault, 6, today);
        assert_eq!(listed.groups.len(), 1, "{:?}", listed.groups);
        let group = &listed.groups[0];
        assert_eq!(group.payee, "工资");
        assert_eq!(group.day, 5);
        assert_eq!(group.next_date, "2026-10-05");
        assert!(group.due);
        assert_eq!(listed.due_count, 1);
    }

    #[test]
    fn a_recorded_month_is_not_due_again() {
        let (_dir, vault) = vault_with(&[
            ("2026-07-05", &salary("2026-07-05")),
            ("2026-08-05", &salary("2026-08-05")),
            ("2026-09-05", &salary("2026-09-05")),
            ("2026-10-05", &salary("2026-10-05")),
        ]);
        let today: NaiveDate = "2026-10-06".parse().unwrap();
        let listed = list(&vault, 6, today);
        assert_eq!(listed.due_count, 0);
        assert_eq!(listed.groups[0].next_date, "2026-11-05");
    }

    #[test]
    fn committing_replays_the_last_occurrence() {
        let (_dir, mut vault) = vault_with(&[
            ("2026-07-05", &salary("2026-07-05")),
            ("2026-08-05", &salary("2026-08-05")),
            ("2026-09-05", &salary("2026-09-05")),
        ]);
        let today: NaiveDate = "2026-09-20".parse().unwrap();
        let listed = list(&vault, 6, today);
        let id = listed.groups[0].id.clone();
        let result = commit(&vault, std::slice::from_ref(&id), &vault.opts).expect("commit");
        assert_eq!(result.written, 1, "{:?}", result.errors);

        let root = vault.root().to_path_buf();
        let opts = vault.opts.clone();
        vault = Vault::load(&root, &opts).expect("reload");
        let created = vault
            .transactions()
            .iter()
            .find(|record| record.txn.date == NaiveDate::from_ymd_opt(2026, 10, 5).unwrap())
            .expect("new transaction");
        assert_eq!(created.txn.payee, "工资");
        assert_eq!(created.values.len(), 2);
        assert!(created.txn.tags.iter().any(
            |tag| tag.key == "recur" && tag.value.as_deref() == Some(&format!("{id}@2026-10"))
        ));
    }

    #[test]
    fn skipping_marks_the_month_as_handled() {
        let (dir, vault) = vault_with(&[
            ("2026-07-05", &salary("2026-07-05")),
            ("2026-08-05", &salary("2026-08-05")),
            ("2026-09-05", &salary("2026-09-05")),
        ]);
        let today: NaiveDate = "2026-09-20".parse().unwrap();
        let listed = list(&vault, 6, today);
        let id = listed.groups[0].id.clone();
        skip(dir.path(), &id, "2026-10").expect("skip");

        let listed = list(&vault, 6, today);
        assert_eq!(listed.due_count, 0);
        assert_eq!(listed.groups[0].done_reason.as_deref(), Some("已忽略"));
        let kept = std::fs::read_to_string(dir.path().join(SKIP_FILE)).expect("skip file");
        assert!(kept.contains(&format!("skip:{id}@2026-10")));
    }
}
