//! Global capture hotkey (default `CmdOrCtrl+Shift+Space`).
//!
//! Only registration lives here: the *behaviour* is owned by the plugin-wide
//! handler installed in `lib.rs`, which compares the pressed chord with the
//! configured one. Keeping a single handler matters — the plugin dispatches the
//! plugin-wide handler *and* any per-shortcut handler for the same keypress, so
//! registering both would toggle the capture window open and closed again in one
//! press (which looks exactly like "the hotkey does nothing").

use std::str::FromStr;

use tauri::AppHandle;
#[cfg(desktop)]
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

/// Parses an accelerator string exactly the way the plugin does, so the UI can
/// reject a chord before it is persisted.
#[cfg(desktop)]
pub fn parse(accelerator: &str) -> Result<Shortcut, String> {
    let trimmed = accelerator.trim();
    Shortcut::from_str(trimmed).map_err(|err| format!("无法识别快捷键「{trimmed}」：{err}"))
}

/// Registers `accelerator` globally. Returns a human readable error when the
/// accelerator is invalid or another application already owns the chord.
#[cfg(desktop)]
pub fn register(app: &AppHandle, accelerator: &str) -> Result<(), String> {
    let trimmed = accelerator.trim();
    let shortcut = parse(trimmed)?;
    app.global_shortcut()
        .register(shortcut)
        .map_err(|err| format!("快捷键「{trimmed}」注册失败，可能已被其他应用占用：{err}"))
}

/// Phones have no global hotkey; the capture sheet is a route in the app.
#[cfg(not(desktop))]
pub fn register(_app: &AppHandle, _accelerator: &str) -> Result<(), String> {
    Ok(())
}

#[cfg(desktop)]
pub fn unregister(app: &AppHandle, accelerator: &str) {
    if let Ok(shortcut) = Shortcut::from_str(accelerator.trim()) {
        let _ = app.global_shortcut().unregister(shortcut);
    }
}

#[cfg(not(desktop))]
pub fn unregister(_app: &AppHandle, _accelerator: &str) {}

/// Swaps the global hotkey. The new chord is registered *before* the old one is
/// released, so a rejected accelerator leaves the previous shortcut working.
pub fn rebind(app: &AppHandle, previous: &str, next: &str) -> Result<(), String> {
    if previous.trim() == next.trim() {
        return Ok(());
    }
    register(app, next)?;
    unregister(app, previous);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn accepts_the_accelerators_the_ui_records() {
        for accelerator in [
            "CmdOrCtrl+Shift+Space",
            "CmdOrCtrl+Alt+KeyK",
            "CmdOrCtrl+Shift+Digit1",
            "Ctrl+Alt+F9",
            "CmdOrCtrl+Shift+ArrowUp",
        ] {
            assert!(parse(accelerator).is_ok(), "{accelerator} should parse");
        }
    }

    #[test]
    fn rejects_unknown_accelerators() {
        for accelerator in ["", "Mod+K", "CmdOrCtrl+", "Shift+NotAKey"] {
            assert!(parse(accelerator).is_err(), "{accelerator:?} should fail");
        }
    }
}
