use ledger_engine::types::VaultCreateOptions;
use ledger_engine::{create_vault, Vault, VaultOptions};

fn main() {
    let dir = std::env::temp_dir().join(format!("mtdebug{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let created = create_vault(&dir, &VaultCreateOptions::default()).unwrap();
    println!("created: {created:?}");
    std::fs::write(
        dir.join("2026/2026-10-25.ledger"),
        "2026-10-25 * 星巴克\n    expenses:food    45.00 CNY\n    assets:cash     -45.00 CNY\n",
    )
    .unwrap();
    let vault = Vault::load(&dir, &VaultOptions::default()).unwrap();
    println!("files: {}", vault.files().len());
    println!("txns: {}", vault.transactions().len());
    for d in vault.diagnostics() {
        println!("diag: {}", d.display());
    }
    for f in vault.files() {
        println!("file entry: {} ({} items)", f.rel, f.journal.items.len());
    }
    println!("--- index.ledger ---");
    println!(
        "{}",
        std::fs::read_to_string(dir.join("index.ledger")).unwrap()
    );
    println!("--- 2026/2026.ledger ---");
    println!(
        "{}",
        std::fs::read_to_string(dir.join("2026/2026.ledger")).unwrap()
    );
}
