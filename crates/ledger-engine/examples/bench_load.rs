//! Generates a synthetic vault with N transactions and times a cold load.
//!
//! ```text
//! cargo run -q --release -p ledger-engine --example bench_load -- 100000
//! ```

use std::time::Instant;

use chrono::NaiveDate;
use ledger_engine::vault::{create_vault, VaultCreateOptions, VaultOptions};
use ledger_engine::writer;
use ledger_engine::Vault;

fn main() {
    let count: usize = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(10_000);
    let dir = std::env::temp_dir().join(format!("moneytracker-bench-{count}"));
    let _ = std::fs::remove_dir_all(&dir);

    let opts = VaultOptions::default();
    let created = create_vault(&dir, &VaultCreateOptions::default()).expect("create vault");
    println!("created {} template files", created.len());

    let accounts = [
        "expenses:food:coffee",
        "expenses:food:lunch",
        "expenses:home:rent",
        "expenses:travel",
        "income:salary",
        "assets:bank:checking",
        "assets:wallet:alipay",
    ];
    let start = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
    let mut written = 0usize;
    for i in 0..count {
        let date = start + chrono::Duration::days((i / 20) as i64);
        let day = writer::ensure_day_file(&dir, date, &opts).expect("day file");
        let amount = (i % 500) as f64 / 7.0 + 1.0;
        let category = accounts[i % accounts.len()];
        let funding = if i % 2 == 0 {
            "assets:wallet:alipay"
        } else {
            "assets:bank:checking"
        };
        let block = format!(
            "{date} * 商户{i}  ; txn:01J{i:022}\n    {category}    {amount:.2} CNY\n    {funding}   -{amount:.2} CNY\n\n"
        );
        let mut text = std::fs::read_to_string(dir.join(&day.rel)).unwrap_or_default();
        text.push_str(&block);
        std::fs::write(dir.join(&day.rel), text).expect("write day file");
        written += 1;
    }
    println!("wrote {written} transactions into {}", dir.display());

    let t0 = Instant::now();
    let vault = Vault::load(&dir, &opts).expect("load");
    let load_ms = t0.elapsed().as_secs_f64() * 1000.0;

    let t1 = Instant::now();
    let snapshot = vault.snapshot();
    let snapshot_ms = t1.elapsed().as_secs_f64() * 1000.0;

    let t2 = Instant::now();
    let dashboard = ledger_engine::report::dashboard(&vault, 12);
    let dashboard_ms = t2.elapsed().as_secs_f64() * 1000.0;

    println!(
        "loaded {} txns from {} files in {:.0} ms ({:.0} us/txn)",
        vault.transactions().len(),
        vault.files().len(),
        load_ms,
        load_ms * 1000.0 / count as f64
    );
    println!(
        "snapshot: {} balance rows, {} accounts, {:.0} ms",
        snapshot.balances.len(),
        snapshot.accounts.len(),
        snapshot_ms
    );
    println!(
        "dashboard: {} months, {} categories, {:.0} ms",
        dashboard.months.len(),
        dashboard.categories.len(),
        dashboard_ms
    );
    // Parse throughput in isolation (sources are already in memory).
    let t3 = Instant::now();
    let mut items = 0usize;
    for file in vault.files() {
        let out = ledger_core::parse_journal(&file.rel, &file.src);
        items += out.journal.items.len();
    }
    let reparse_ms = t3.elapsed().as_secs_f64() * 1000.0;
    println!(
        "re-parse of every source: {items} items in {reparse_ms:.0} ms ({:.1} MB/s)",
        vault.files().iter().map(|f| f.src.len()).sum::<usize>() as f64 / 1e6 * 1000.0 / reparse_ms
    );

    println!(
        "diagnostics: {}, warnings: {:?}",
        vault.diagnostics().len(),
        vault.warnings
    );

    let _ = std::fs::remove_dir_all(&dir);
}
