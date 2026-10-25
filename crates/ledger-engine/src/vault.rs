//! Vault layout, templates and path conventions.

use std::path::{Path, PathBuf};

use chrono::NaiveDate;

pub use crate::types::VaultCreateOptions;
use crate::EngineError;

pub const INDEX_FILE: &str = "index.ledger";
pub const RULES_FILE: &str = "rules.ledger";
pub const PRICES_FILE: &str = "prices.ledger";
pub const BUDGETS_FILE: &str = "budgets.ledger";
pub const ACCOUNTS_FILE: &str = "accounts.ledger";
pub const BAK_DIR: &str = ".bak";

#[derive(Debug, Clone)]
pub struct VaultOptions {
    pub base_commodity: String,
    pub base_precision: u32,
    pub default_funding: String,
    pub default_unclassified: String,
    pub fx_enabled: bool,
    /// Top level account name -> hledger account type (A/L/E/R/X).
    pub account_type_map: Vec<(String, String)>,
    pub max_backups: usize,
}

impl Default for VaultOptions {
    fn default() -> Self {
        Self {
            base_commodity: "CNY".to_string(),
            base_precision: 2,
            default_funding: "assets:cash".to_string(),
            default_unclassified: "expenses:unclassified".to_string(),
            fx_enabled: false,
            account_type_map: vec![
                ("assets".into(), "A".into()),
                ("liabilities".into(), "L".into()),
                ("equity".into(), "E".into()),
                ("income".into(), "R".into()),
                ("revenues".into(), "R".into()),
                ("revenue".into(), "R".into()),
                ("expenses".into(), "X".into()),
            ],
            max_backups: 200,
        }
    }
}

pub fn year_dir(date: NaiveDate) -> String {
    format!("{}", date.format("%Y"))
}

pub fn year_path(date: NaiveDate) -> PathBuf {
    PathBuf::from(format!(
        "{}/{}.ledger",
        date.format("%Y"),
        date.format("%Y")
    ))
}

pub fn month_path(date: NaiveDate) -> PathBuf {
    PathBuf::from(format!(
        "{}/{}.ledger",
        date.format("%Y"),
        date.format("%Y-%m")
    ))
}

pub fn day_path(date: NaiveDate) -> PathBuf {
    PathBuf::from(format!(
        "{}/{}.ledger",
        date.format("%Y"),
        date.format("%Y-%m-%d")
    ))
}

pub fn render_index(opts: &VaultOptions, year: i32) -> String {
    format!(
        "; MoneyTracker vault — everything hledger needs lives here.
;
; rules.ledger and goals.ledger are intentionally NOT included: MoneyTracker
; expands rules at write time and treats goals as commentary plus debt
; parameters, so an external `hledger -f index.ledger ...` sees the same final
; transactions without double counting.
;
; recurring.ledger IS included: hledger ignores `~` blocks unless you ask for
; --forecast. MoneyTracker's own budget, forecast and payoff reports skip this
; file, so a declared schedule never becomes an envelope.

commodity 1,000.00 {base}

include {accounts}
include {prices}
include {budgets}
include {recurring}
include {year}/{year}.ledger
",
        base = opts.base_commodity,
        accounts = ACCOUNTS_FILE,
        prices = PRICES_FILE,
        budgets = BUDGETS_FILE,
        recurring = crate::schedule::SCHEDULE_FILE,
        year = year,
    )
}

fn render_accounts(opts: &VaultOptions) -> String {
    format!(
        "; Declared accounts. The `type:` tag is authoritative for reports.
account assets:cash            ; type:A
account assets:bank            ; type:A
account assets:wallet          ; type:A
account liabilities:creditcard ; type:L
account income:salary          ; type:R
account expenses:food          ; type:X
account {unclassified}  ; type:X
account equity:opening         ; type:E
",
        unclassified = opts.default_unclassified
    )
}

fn render_prices() -> String {
    "; Exchange rates. `P` directives are the ONLY source of truth for FX.
; Example: P 2026-10-25 USD 7.12 CNY
"
    .to_string()
}

fn render_budgets() -> String {
    "; Periodic transactions / budgets (consumed only for budgets & forecasts).
; The positive leg is the envelope — an `expenses:` account is a spending budget,
; an `assets:` / `liabilities:` account is a savings goal.
; Example:
; ~ monthly
;     expenses:food               1500.00 CNY
;     assets:cash
; ~ monthly
;     assets:invest:retirement     500.00 CNY
;     assets:cash
"
    .to_string()
}

fn render_rules() -> String {
    "; MoneyTracker rules — this file is read by the app and is NOT included by
; index.ledger, so hledger never double-applies them.
; Syntax:  = <query>  ; rule:<id> order:<n> enabled:<bool> from:<account>
;             expenses:food:coffee   70.00%
;             expenses:food:beans    30.00%
"
    .to_string()
}

/// Create the vault template. Never overwrites existing files.
pub fn create_vault(root: &Path, opts: &VaultCreateOptions) -> Result<Vec<String>, EngineError> {
    let options = VaultOptions {
        base_commodity: opts
            .base_commodity
            .clone()
            .unwrap_or_else(|| VaultOptions::default().base_commodity),
        default_funding: opts
            .default_funding
            .clone()
            .unwrap_or_else(|| VaultOptions::default().default_funding),
        default_unclassified: opts
            .default_unclassified
            .clone()
            .unwrap_or_else(|| VaultOptions::default().default_unclassified),
        ..VaultOptions::default()
    };
    std::fs::create_dir_all(root).map_err(|e| EngineError::Io(e.to_string()))?;

    let year = chrono::Local::now().date_naive().format("%Y").to_string();
    let year_num: i32 = year.parse().unwrap_or(2026);
    let with_template = opts.template.unwrap_or(true);

    let mut created = Vec::new();
    let files: Vec<(PathBuf, String)> = if with_template {
        vec![
            (PathBuf::from(INDEX_FILE), render_index(&options, year_num)),
            (PathBuf::from(ACCOUNTS_FILE), render_accounts(&options)),
            (PathBuf::from(PRICES_FILE), render_prices()),
            (PathBuf::from(BUDGETS_FILE), render_budgets()),
            (PathBuf::from(RULES_FILE), render_rules()),
            (
                PathBuf::from(crate::schedule::SCHEDULE_FILE),
                crate::schedule::render_starter(&options.base_commodity),
            ),
            (
                PathBuf::from(format!("{year}/{year}.ledger")),
                format!("; {}\n", year),
            ),
        ]
    } else {
        vec![(PathBuf::from(INDEX_FILE), render_index(&options, year_num))]
    };

    for (rel, content) in files {
        let abs = root.join(&rel);
        if abs.exists() {
            continue;
        }
        if let Some(parent) = abs.parent() {
            std::fs::create_dir_all(parent).map_err(|e| EngineError::Io(e.to_string()))?;
        }
        std::fs::write(&abs, content).map_err(|e| EngineError::Io(e.to_string()))?;
        created.push(rel.to_string_lossy().to_string());
    }

    let gitignore = root.join(".gitignore");
    if with_template && !gitignore.exists() {
        std::fs::write(&gitignore, ".bak/\n").map_err(|e| EngineError::Io(e.to_string()))?;
        created.push(".gitignore".to_string());
    }

    Ok(created)
}

/// Validate that a directory looks like a MoneyTracker vault.
pub fn validate_vault(root: &Path) -> Result<(), EngineError> {
    if !root.is_dir() {
        return Err(EngineError::Vault(format!(
            "{} is not a directory",
            root.display()
        )));
    }
    if !root.join(INDEX_FILE).is_file() {
        return Err(EngineError::Vault(format!(
            "{} does not contain {INDEX_FILE}",
            root.display()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn creates_template_without_rules_include() {
        let dir = tempdir().unwrap();
        let created = create_vault(
            dir.path(),
            &VaultCreateOptions {
                base_commodity: Some("CNY".into()),
                default_funding: None,
                default_unclassified: None,
                template: Some(true),
            },
        )
        .unwrap();
        assert!(created.contains(&"index.ledger".to_string()));
        let index = std::fs::read_to_string(dir.path().join(INDEX_FILE)).unwrap();
        assert!(index.contains("commodity 1,000.00 CNY"));
        assert!(index.contains("include accounts.ledger"));
        assert!(
            !index.lines().any(|l| {
                let l = l.trim_start();
                l.starts_with("include rules.ledger") || l.starts_with("!include rules.ledger")
            }),
            "index.ledger must not include rules.ledger:\n{index}"
        );
        assert!(dir.path().join(RULES_FILE).exists());
        assert!(dir.path().join(".gitignore").exists());
    }

    #[test]
    fn does_not_overwrite_existing_files() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join(INDEX_FILE), "; mine\n").unwrap();
        create_vault(
            dir.path(),
            &VaultCreateOptions {
                base_commodity: None,
                default_funding: None,
                default_unclassified: None,
                template: Some(true),
            },
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join(INDEX_FILE)).unwrap(),
            "; mine\n"
        );
    }

    #[test]
    fn path_layout_is_layered() {
        let d = NaiveDate::from_ymd_opt(2026, 10, 25).unwrap();
        assert_eq!(year_path(d), PathBuf::from("2026/2026.ledger"));
        assert_eq!(month_path(d), PathBuf::from("2026/2026-10.ledger"));
        assert_eq!(day_path(d), PathBuf::from("2026/2026-10-25.ledger"));
    }
}
