//! Filesystem watcher: external edits reload the vault and notify the UI.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use ledger_engine::Vault;
use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use tauri::{AppHandle, Manager};
use tauri_specta::Event as _;

use crate::state::AppState;

/// Ledger files that changed on disk since the vault snapshot was taken.
fn changed_on_disk(vault: &Vault) -> Option<Vec<String>> {
    let mut touched = Vec::new();
    for file in vault.files() {
        match ledger_engine::writer::file_state(&file.abs) {
            Ok(state) if state.hash == file.state.hash => {}
            _ => touched.push(file.rel.clone()),
        }
    }
    (!touched.is_empty()).then_some(touched)
}

pub fn watch(app: AppHandle, root: PathBuf) {
    unwatch(&app);

    let (tx, rx) = mpsc::channel::<()>();
    let watcher = RecommendedWatcher::new(
        move |res: notify::Result<notify::Event>| {
            if let Ok(event) = res {
                let relevant = event.paths.iter().any(|p| {
                    !p.to_string_lossy().contains("/.bak/")
                        && p.extension().map(|e| e == "ledger").unwrap_or(false)
                });
                if relevant {
                    let _ = tx.send(());
                }
            }
        },
        Config::default(),
    );
    let mut watcher = match watcher {
        Ok(w) => w,
        Err(_) => return,
    };
    if watcher
        .watch(Path::new(&root), RecursiveMode::Recursive)
        .is_err()
    {
        return;
    }

    *app.state::<AppState>()
        .watcher
        .lock()
        .expect("watcher poisoned") = Some(watcher);

    // Debounce: coalesce bursts, then reload once.
    std::thread::spawn(move || {
        while rx.recv().is_ok() {
            std::thread::sleep(Duration::from_millis(300));
            while rx.try_recv().is_ok() {}
            let state = app.state::<AppState>();
            let opts = state.settings().vault_options();
            let Ok(current) = state.vault() else { continue };
            if changed_on_disk(&current).is_none() {
                // Our own write: the in-memory vault already matches the disk.
                continue;
            }
            // Same entry point as the commands, so a watcher reload cannot
            // interleave with, and clobber, one triggered by a write.
            state.reload_from_disk(&opts);
            let _ = crate::commands::LedgerChanged {
                paths: vec!["*".to_string()],
                reason: "external_change".to_string(),
            }
            .emit(&app);
        }
    });
}

pub fn unwatch(app: &AppHandle) {
    let state = app.state::<AppState>();
    *state.watcher.lock().expect("watcher poisoned") = None;
}
