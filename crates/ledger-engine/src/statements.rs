//! Financial statements: balance sheet, income statement and the net-worth bridge.
//!
//! Everything is derived from the indexed transactions, so the statements agree
//! with the dashboard and with `hledger balancesheet` / `hledger incomestatement`
//! by construction. Transactions with a missing FX rate are excluded (and
//! counted) rather than converted at 1:1.

use std::collections::BTreeMap;

use chrono::NaiveDate;
use rust_decimal::Decimal;

use crate::load::Vault;
use crate::types::{
    AmountDto, BalanceSheetDto, BridgeDto, IncomeStatementDto, SectionDto, StatementRowDto,
};

fn money(vault: &Vault, quantity: Decimal) -> AmountDto {
    let (commodity, precision) = vault.base_commodity();
    crate::amounts::decimal_to_dto(quantity, commodity, precision)
}

/// Signed base-commodity balance per account, restricted to `date < up_to`
/// (`None` = all transactions).
fn sums_until(vault: &Vault, up_to: Option<NaiveDate>) -> BTreeMap<String, Decimal> {
    let mut sums: BTreeMap<String, Decimal> = BTreeMap::new();
    for rec in vault.transactions() {
        if let Some(limit) = up_to {
            if rec.txn.date >= limit {
                continue;
            }
        }
        if rec.fx_missing {
            continue;
        }
        for value in &rec.values {
            let Some(base) = value.base else { continue };
            *sums.entry(value.account.clone()).or_insert(Decimal::ZERO) += base;
        }
    }
    sums
}

fn short_label(account: &str) -> String {
    match account.split_once(':') {
        Some((_, rest)) if !rest.is_empty() => rest.to_string(),
        _ => account.to_string(),
    }
}

fn row(vault: &Vault, account: &str, amount: Decimal) -> StatementRowDto {
    StatementRowDto {
        account: account.to_string(),
        label: short_label(account),
        unit: money(vault, amount),
        hint: None,
    }
}

/// Build a section from signed debit-positive amounts, flipping credit accounts
/// so the UI can show liabilities, equity and income as positive numbers.
fn section(
    vault: &Vault,
    sums: &BTreeMap<String, Decimal>,
    title: &str,
    kinds: &[char],
) -> SectionDto {
    let mut entries: Vec<(String, Decimal)> = sums
        .iter()
        .filter(|(account, amount)| kinds.contains(&vault.kind_for(account)) && !amount.is_zero())
        .map(|(account, amount)| {
            let value = match vault.kind_for(account) {
                'L' | 'E' | 'R' => -*amount,
                _ => *amount,
            };
            (account.clone(), value)
        })
        .collect();
    entries.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
    let total: Decimal = entries.iter().map(|(_, value)| *value).sum();
    SectionDto {
        title: title.to_string(),
        rows: entries
            .into_iter()
            .map(|(account, value)| row(vault, &account, value))
            .collect(),
        total: money(vault, total),
    }
}

fn total_of(section: &SectionDto) -> Decimal {
    Decimal::from_str_exact(&section.total.quantity).unwrap_or_default()
}

/// Balance sheet as of the end of `as_of` (inclusive).
pub fn balance_sheet(vault: &Vault, as_of: NaiveDate) -> BalanceSheetDto {
    let sums = sums_until(vault, as_of.succ_opt());
    let assets = section(vault, &sums, "资产", &['A']);
    let liabilities = section(vault, &sums, "负债", &['L']);
    let equity = section(vault, &sums, "权益", &['E']);
    let income = section(vault, &sums, "收入", &['R']);
    let expense = section(vault, &sums, "支出", &['X']);
    let cumulative_net = total_of(&income) - total_of(&expense);
    let note = match vault.info_dto().first_date {
        Some(first) => {
            let first_date = ledger_core::parser::parse_date_str(&first);
            if first_date.map(|d| d > as_of).unwrap_or(false) {
                Some(format!("账本从 {first} 开始，此日期之前没有任何交易"))
            } else {
                None
            }
        }
        None => Some("账本里还没有交易".to_string()),
    };
    BalanceSheetDto {
        as_of: as_of.to_string(),
        assets: assets.clone(),
        liabilities: liabilities.clone(),
        equity,
        income,
        expense,
        cumulative_net: money(vault, cumulative_net),
        net_worth: money(vault, total_of(&assets) - total_of(&liabilities)),
        note,
    }
}

/// Income statement for `from..=to`.
pub fn income_statement(vault: &Vault, from: NaiveDate, to: NaiveDate) -> IncomeStatementDto {
    let mut income: BTreeMap<String, Decimal> = BTreeMap::new();
    let mut expense: BTreeMap<String, Decimal> = BTreeMap::new();
    for rec in vault.transactions() {
        if rec.txn.date < from || rec.txn.date > to || rec.fx_missing {
            continue;
        }
        for value in &rec.values {
            let Some(base) = value.base else { continue };
            match value.kind {
                'R' => *income.entry(value.account.clone()).or_insert(Decimal::ZERO) += -base,
                'X' => {
                    *expense
                        .entry(value.account.clone())
                        .or_insert(Decimal::ZERO) += base
                }
                _ => {}
            }
        }
    }
    let to_section = |title: &str, map: BTreeMap<String, Decimal>| {
        let mut entries: Vec<(String, Decimal)> = map
            .into_iter()
            .filter(|(_, amount)| !amount.is_zero())
            .collect();
        entries.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.cmp(&b.0))
        });
        let total: Decimal = entries.iter().map(|(_, value)| *value).sum();
        SectionDto {
            title: title.to_string(),
            rows: entries
                .into_iter()
                .map(|(account, value)| row(vault, &account, value))
                .collect(),
            total: money(vault, total),
        }
    };
    let income_section = to_section("收入", income);
    let expense_section = to_section("支出", expense);
    IncomeStatementDto {
        from: from.to_string(),
        to: to.to_string(),
        net: money(
            vault,
            total_of(&income_section) - total_of(&expense_section),
        ),
        income: income_section,
        expenses: expense_section,
    }
}

fn net_worth_until(vault: &Vault, up_to: Option<NaiveDate>) -> Decimal {
    let sums = sums_until(vault, up_to);
    // Net worth = assets + liabilities, both as signed ledger balances
    // (a liability balance is already negative).
    sums.iter()
        .filter(|(account, _)| matches!(vault.kind_for(account), 'A' | 'L'))
        .map(|(_, raw)| *raw)
        .sum()
}

/// Why net worth moved: opening + income − expense + valuation change.
///
/// The residual is the revaluation of foreign-currency balances between the two
/// dates, because each posting is converted with the rate of its own day.
pub fn net_worth_bridge(vault: &Vault, from: NaiveDate, to: NaiveDate) -> BridgeDto {
    let opening = net_worth_until(vault, Some(from));
    let closing = net_worth_until(vault, to.succ_opt());
    let mut income = Decimal::ZERO;
    let mut expense = Decimal::ZERO;
    for rec in vault.transactions() {
        if rec.txn.date < from || rec.txn.date > to || rec.fx_missing {
            continue;
        }
        for value in &rec.values {
            let Some(base) = value.base else { continue };
            match value.kind {
                'R' => income += -base,
                'X' => expense += base,
                _ => {}
            }
        }
    }
    let explained = opening + income - expense;
    BridgeDto {
        from: from.to_string(),
        to: to.to_string(),
        opening: money(vault, opening),
        income: money(vault, income),
        expense: money(vault, expense),
        valuation: money(vault, closing - explained),
        closing: money(vault, closing),
        reconciled: (closing - explained).abs() < Decimal::new(1, 2),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::VaultCreateOptions;
    use crate::vault::{create_vault, VaultOptions};
    use tempfile::tempdir;

    fn vault() -> (tempfile::TempDir, Vault) {
        let dir = tempdir().unwrap();
        create_vault(
            dir.path(),
            &VaultCreateOptions {
                template: Some(true),
                ..Default::default()
            },
        )
        .unwrap();
        std::fs::write(
            dir.path().join("accounts.ledger"),
            "account assets:cash ; type:A\naccount assets:bank ; type:A\naccount liabilities:card ; type:L\naccount equity:opening ; type:E\naccount income:salary ; type:R\naccount expenses:food ; type:X\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("2026/2026.ledger"),
            "include 2026-01.ledger\ninclude 2026-02.ledger\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("2026/2026-01.ledger"),
            "include 2026-01-01.ledger\ninclude 2026-01-20.ledger\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("2026/2026-02.ledger"),
            "include 2026-02-10.ledger\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("2026/2026-01-01.ledger"),
            "2026-01-01 opening\n    assets:cash    5000.00 CNY\n    equity:opening  -5000.00 CNY\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("2026/2026-01-20.ledger"),
            "2026-01-20 salary\n    assets:bank    3000.00 CNY\n    income:salary  -3000.00 CNY\n\n2026-01-20 lunch\n    expenses:food   120.00 CNY\n    liabilities:card -120.00 CNY\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("2026/2026-02-10.ledger"),
            "2026-02-10 dinner\n    expenses:food   300.00 CNY\n    assets:cash   -300.00 CNY\n",
        )
        .unwrap();
        let vault = Vault::load(dir.path(), &VaultOptions::default()).unwrap();
        (dir, vault)
    }

    fn date(s: &str) -> NaiveDate {
        ledger_core::parser::parse_date_str(s).unwrap()
    }

    fn qty(amount: &AmountDto) -> Decimal {
        Decimal::from_str_exact(&amount.quantity).unwrap_or_default()
    }

    #[test]
    fn balance_sheet_satisfies_the_accounting_identity() {
        let (_dir, vault) = vault();
        let sheet = balance_sheet(&vault, date("2026-02-28"));
        assert_eq!(sheet.assets.total.quantity, "7700.00");
        assert_eq!(sheet.liabilities.total.quantity, "120.00");
        assert_eq!(sheet.equity.total.quantity, "5000.00");
        assert_eq!(sheet.cumulative_net.quantity, "2580.00");
        assert_eq!(sheet.net_worth.quantity, "7580.00");
        let assets = total_of(&sheet.assets);
        let liabilities = total_of(&sheet.liabilities);
        let equity = total_of(&sheet.equity);
        let net = Decimal::from_str_exact(&sheet.cumulative_net.quantity).unwrap();
        assert_eq!(assets - liabilities, equity + net);
    }

    #[test]
    fn balance_sheet_respects_as_of() {
        let (_dir, vault) = vault();
        let before = balance_sheet(&vault, date("2025-12-31"));
        assert_eq!(qty(&before.assets.total), Decimal::ZERO);
        assert!(before.note.is_some());
        let mid = balance_sheet(&vault, date("2026-01-20"));
        assert_eq!(mid.assets.total.quantity, "8000.00");
        assert_eq!(mid.liabilities.total.quantity, "120.00");
    }

    #[test]
    fn income_statement_matches_the_month_totals() {
        let (_dir, vault) = vault();
        let stmt = income_statement(&vault, date("2026-01-01"), date("2026-01-31"));
        assert_eq!(stmt.income.total.quantity, "3000.00");
        assert_eq!(stmt.expenses.total.quantity, "120.00");
        assert_eq!(stmt.net.quantity, "2880.00");
        let feb = income_statement(&vault, date("2026-02-01"), date("2026-02-28"));
        assert_eq!(qty(&feb.income.total), Decimal::ZERO);
        assert_eq!(feb.expenses.total.quantity, "300.00");
        assert_eq!(feb.net.quantity, "-300.00");
    }

    #[test]
    fn bridge_explains_net_worth_changes() {
        let (_dir, vault) = vault();
        let bridge = net_worth_bridge(&vault, date("2026-01-20"), date("2026-02-28"));
        assert_eq!(bridge.opening.quantity, "5000.00");
        assert_eq!(bridge.closing.quantity, "7580.00");
        assert_eq!(bridge.income.quantity, "3000.00");
        assert_eq!(bridge.expense.quantity, "420.00");
        assert_eq!(bridge.valuation.quantity, "0.00");
        assert!(bridge.reconciled);
    }

    #[test]
    fn rows_are_sorted_by_size_and_hide_empty_parents() {
        let (_dir, vault) = vault();
        let stmt = income_statement(&vault, date("2026-01-01"), date("2026-02-28"));
        let accounts: Vec<&str> = stmt
            .expenses
            .rows
            .iter()
            .map(|r| r.account.as_str())
            .collect();
        assert_eq!(accounts, vec!["expenses:food"]);
        assert_eq!(stmt.expenses.rows[0].label, "food");
    }
}
