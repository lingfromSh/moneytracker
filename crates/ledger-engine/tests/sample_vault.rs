//! End-to-end check against the checked-in sample vault in `fixtures/vault/sample`.
//! This is the file `hledger -f index.ledger balance` is compared against manually.

use std::path::{Path, PathBuf};

use ledger_engine::report;
use ledger_engine::types::QueryRequest;
use ledger_engine::{Vault, VaultOptions};
use rust_decimal::Decimal;
use std::str::FromStr;

fn find<'a>(
    nodes: &'a [ledger_engine::types::AccountNodeDto],
    full: &str,
) -> Option<&'a ledger_engine::types::AccountNodeDto> {
    for node in nodes {
        if node.full == full {
            return Some(node);
        }
        if let Some(hit) = find(&node.children, full) {
            return Some(hit);
        }
    }
    None
}

fn sample() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/vault/sample")
}

#[test]
fn sample_vault_loads_without_errors() {
    let vault = Vault::load(&sample(), &VaultOptions::default()).expect("sample vault loads");
    let errors: Vec<String> = vault
        .diagnostics()
        .iter()
        .filter(|d| d.severity == ledger_core::Severity::Error)
        .map(|d| d.display())
        .collect();
    assert!(
        errors.is_empty(),
        "sample vault has parse errors: {errors:#?}"
    );
    assert_eq!(vault.transactions().len(), 7, "expected seven transactions");
    assert!(vault.transactions().iter().all(|t| t.balanced));
    assert_eq!(vault.missing_fx_count(), 0);
    // Accounts declared in accounts.ledger keep their declared kinds.
    let tree = vault.account_tree();
    let cash = find(&tree, "assets:cash").expect("assets:cash exists");
    assert_eq!(cash.kind, "A");
    assert!(find(&tree, "expenses:food:coffee").is_some());
}

#[test]
fn sample_vault_balances_match_plain_arithmetic() {
    let vault = Vault::load(&sample(), &VaultOptions::default()).unwrap();
    let d = |s: &str| Decimal::from_str(s).unwrap();
    // income 24,000 − rent 6,400 − coffee 45 − beans 88 − travel 712 (USD 100 @ 7.12)
    let expected = d("24000") - d("6400") - d("45") - d("88") - d("712");
    let total: Decimal = vault.net_worth().iter().map(|a| d(&a.quantity)).sum();
    assert_eq!(total, expected, "net worth should equal {expected}");
}

#[test]
fn sample_vault_reports_are_populated() {
    let vault = Vault::load(&sample(), &VaultOptions::default()).unwrap();
    let dash = report::dashboard(&vault, 6);
    assert_eq!(dash.kpis.month, "2026-10");
    assert!(!dash.months.is_empty());
    assert!(!dash.categories.is_empty());
    assert!(!dash.sankey.nodes.is_empty());
    assert_eq!(dash.fx_missing.len(), 0);

    let page = report::query_txns(
        &vault,
        &QueryRequest {
            query: Some("acct:coffee".into()),
            offset: 0,
            limit: 20,
            order: None,
            date_from: None,
            date_to: None,
        },
    )
    .unwrap();
    assert_eq!(
        page.total, 1,
        "only the Starbucks transaction touches coffee"
    );
    assert_eq!(page.items[0].payee, "星巴克");
}

#[test]
fn sample_vault_prices_drive_conversion() {
    let vault = Vault::load(&sample(), &VaultOptions::default()).unwrap();
    let travel = vault
        .transactions()
        .iter()
        .find(|t| t.txn.payee == "东京出差")
        .expect("travel transaction");
    // 100 USD @@ 712 CNY is recorded as a total price, so the base value is exactly 712.
    assert_eq!(travel.base_value, Some(Decimal::from_str("712").unwrap()));
}

/// A node's `total` must equal its own postings plus every descendant `total`,
/// and a leaf must report the same number in `own` and `total`. The leaf used to
/// be credited twice, which made every account row read double its balance while
/// the parent rows and the kind totals stayed correct.
fn assert_tree_totals(nodes: &[ledger_engine::types::AccountNodeDto]) {
    let value = |amounts: &[ledger_engine::types::AmountDto]| -> Decimal {
        amounts
            .first()
            .and_then(|a| a.quantity.parse().ok())
            .unwrap_or_default()
    };
    for node in nodes {
        let own = value(&node.own);
        let total = value(&node.total);
        let children: Decimal = node.children.iter().map(|c| value(&c.total)).sum();
        assert!(
            (total - (own + children)).abs() < Decimal::new(1, 2),
            "{} total {total} != own {own} + children {children}",
            node.full
        );
        if node.children.is_empty() {
            assert_eq!(own, total, "leaf {} reports own != total", node.full);
        }
        assert_tree_totals(&node.children);
    }
}

#[test]
fn account_tree_totals_match_their_children() {
    let vault = Vault::load(&sample(), &VaultOptions::default()).unwrap();
    assert_tree_totals(&vault.account_tree());
}

#[test]
fn sample_vault_budgets_come_from_budgets_ledger() {
    let vault = Vault::load(&sample(), &VaultOptions::default()).expect("sample vault loads");
    let budgets = ledger_engine::report::budgets(&vault);
    let accounts: Vec<&str> = budgets.iter().map(|b| b.account.as_str()).collect();
    assert!(
        accounts.contains(&"expenses:home:rent"),
        "budgets parsed from budgets.ledger: {accounts:?}"
    );
}
