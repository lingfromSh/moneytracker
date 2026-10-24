//! Global capture hotkey (default `Cmd+Shift+Space`).

use std::str::FromStr;

use tauri::AppHandle;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::window;

pub fn register(app: &AppHandle, accelerator: &str) {
    let Ok(shortcut) = Shortcut::from_str(accelerator) else {
        eprintln!("moneytracker: invalid shortcut {accelerator:?}");
        return;
    };
    let handle = app.clone();
    let result = app
        .global_shortcut()
        .on_shortcut(shortcut, move |_app, _sc, event| {
            if event.state() == ShortcutState::Pressed {
                window::toggle_capture(&handle);
            }
        });
    if let Err(err) = result {
        eprintln!("moneytracker: cannot register {accelerator}: {err}");
    }
}

pub fn rebind(app: &AppHandle, previous: &str, next: &str) {
    if let Ok(shortcut) = Shortcut::from_str(previous) {
        let _ = app.global_shortcut().unregister(shortcut);
    }
    register(app, next);
}
