//! Differential check against the real `hledger` binary.
//!
//! `hledger` is optional: when it is not installed the tests report that they were
//! skipped instead of failing the suite.

use std::collections::BTreeMap;
use std::process::Command;

use ledger_engine::vault::VaultOptions;
use ledger_engine::Vault;

fn sample_vault() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/vault/sample")
        .canonicalize()
        .expect("sample vault")
}

fn hledger_available() -> bool {
    Command::new("hledger")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// `account -> "quantity commodity"` as printed by `hledger balance --flat --no-total -O csv`.
fn hledger_balances(path: &std::path::Path) -> BTreeMap<String, String> {
    let output = Command::new("hledger")
        .arg("-f")
        .arg(path)
        .args(["balance", "--flat", "--no-total", "-O", "csv"])
        .output()
        .expect("run hledger");
    assert!(
        output.status.success(),
        "hledger failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);
    let mut rows = BTreeMap::new();
    for line in text.lines().skip(1) {
        let fields = csv_fields(line);
        if fields.len() != 2 {
            continue;
        }
        let (account, balance) = (fields[0].clone(), fields[1].clone());
        // `-88.00 CNY` / `100.00 USD @@ ...` — keep the first amount only.
        let normalised = balance.trim().to_string();
        rows.insert(account, normalised);
    }
    rows
}

fn csv_fields(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for ch in line.chars() {
        match ch {
            '"' => quoted = !quoted,
            ',' if !quoted => fields.push(std::mem::take(&mut current)),
            _ => current.push(ch),
        }
    }
    fields.push(current);
    fields
}

/// The engine prints `-88.00 CNY`; strip thousands separators so both sides compare.
fn normalise_amount(text: &str) -> String {
    text.chars().filter(|c| *c != ',').collect::<String>()
}

#[test]
fn engine_balances_match_hledger() {
    if !hledger_available() {
        eprintln!("hledger not installed — skipping the differential balance check");
        return;
    }
    let vault_path = sample_vault();
    let opts = VaultOptions::default();
    let vault = Vault::load(&vault_path, &opts).expect("load sample vault");

    let expected = hledger_balances(&vault_path.join("index.ledger"));
    assert!(!expected.is_empty(), "hledger produced no balances");

    let mut actual: BTreeMap<String, String> = BTreeMap::new();
    for row in vault.balances() {
        let key = row.account.clone();
        let value = normalise_amount(&row.display);
        match actual.get(&key) {
            // A multi-commodity account prints one row per commodity.
            Some(existing) => {
                actual.insert(key, format!("{existing} + {value}"));
            }
            None => {
                actual.insert(key, value);
            }
        }
    }

    for (account, expected_amount) in expected.clone() {
        let expected_amount = normalise_amount(&expected_amount);
        let actual_amount = actual
            .get(&account)
            .unwrap_or_else(|| panic!("engine is missing account {account}"));
        assert_eq!(
            *actual_amount, expected_amount,
            "balance mismatch for {account}"
        );
    }
    for account in actual.keys() {
        if !expected.contains_key(account) {
            // hledger hides zero balances by default; anything non-zero must match.
            let amount = &actual[account];
            assert!(
                amount.contains("0.00") || amount.contains("0.0000"),
                "engine reports {account} = {amount} but hledger does not list it"
            );
        }
    }
}

#[test]
fn hledger_accepts_a_vault_written_by_the_engine() {
    if !hledger_available() {
        eprintln!("hledger not installed — skipping the differential check");
        return;
    }
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("vault");
    let opts = VaultOptions::default();
    ledger_engine::vault::create_vault(&root, &Default::default()).expect("create vault");
    let vault = Vault::load(&root, &opts).expect("load");

    let id = ledger_engine::capture::new_txn_id();
    let analysis = ledger_engine::capture::analyze(&vault, "咖啡 4.5 @微信 +餐饮");
    assert!(analysis.errors.is_empty(), "{:?}", analysis.errors);
    let expanded = ledger_engine::capture::expand(&vault, &analysis.parsed);
    let committed =
        ledger_engine::capture::commit_to_day_file(&vault, &expanded, &id, &opts).expect("commit");
    assert!(committed.rel.ends_with(".ledger"));

    let check = Command::new("hledger")
        .arg("-f")
        .arg(root.join("index.ledger"))
        .arg("check")
        .output()
        .expect("run hledger");
    assert!(
        check.status.success(),
        "hledger rejected the generated vault: {}{}",
        String::from_utf8_lossy(&check.stdout),
        String::from_utf8_lossy(&check.stderr)
    );

    let balance = Command::new("hledger")
        .arg("-f")
        .arg(root.join("index.ledger"))
        .args(["balance", "--flat", "--no-total"])
        .output()
        .expect("run hledger");
    let text = String::from_utf8_lossy(&balance.stdout);
    assert!(
        text.contains("expenses:餐饮") && text.contains("assets:微信"),
        "unexpected hledger output: {text}"
    );
    assert!(
        text.contains("4.50 CNY"),
        "hledger did not see the captured amount: {text}"
    );
}
