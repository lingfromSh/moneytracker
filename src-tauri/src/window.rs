//! Window helpers: the capture palette and the main window.
//!
//! The capture palette is a floating always-on-top window, which is a desktop
//! idea; on a phone the capture sheet is a normal route inside the main window,
//! so the helpers below degrade to no-ops there.

use tauri::{AppHandle, Manager};
#[cfg(desktop)]
use tauri::{WebviewUrl, WebviewWindowBuilder};

#[cfg_attr(not(desktop), allow(dead_code))]
pub const MAIN: &str = "main";
pub const CAPTURE: &str = "capture";

#[cfg(desktop)]
pub fn show_capture(app: &AppHandle) {
    let Some(window) = app.get_webview_window(CAPTURE) else {
        return;
    };
    let _ = window.show();
    let _ = window.set_focus();
    let _ = window.center();
}

#[cfg(desktop)]
pub fn hide_capture(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(CAPTURE) {
        let _ = window.hide();
    }
}

#[cfg(desktop)]
pub fn toggle_capture(app: &AppHandle) {
    let Some(window) = app.get_webview_window(CAPTURE) else {
        let _ = create_capture_window(app);
        return;
    };
    match window.is_visible() {
        Ok(true) => {
            let _ = window.hide();
        }
        _ => {
            let _ = window.show();
            let _ = window.set_focus();
            let _ = window.center();
        }
    }
}

#[cfg(desktop)]
pub fn create_capture_window(app: &AppHandle) -> tauri::Result<()> {
    let window =
        WebviewWindowBuilder::new(app, CAPTURE, WebviewUrl::App("index.html#/capture".into()))
            .title("MoneyTracker Capture")
            .inner_size(640.0, 208.0)
            .resizable(false)
            .decorations(false)
            .transparent(true)
            .always_on_top(true)
            .skip_taskbar(true)
            .visible(false)
            .center()
            .build()?;
    let _ = window.set_focus();
    Ok(())
}

#[cfg_attr(not(desktop), allow(dead_code))]
pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN) {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

// The mobile shells keep the same command surface, so these exist only to make
// the desktop calls compile — nothing on a phone ever invokes them.
#[cfg(not(desktop))]
#[allow(dead_code)]
pub fn show_capture(_app: &AppHandle) {}

#[cfg(not(desktop))]
#[allow(dead_code)]
pub fn hide_capture(_app: &AppHandle) {}

#[cfg(not(desktop))]
#[allow(dead_code)]
pub fn toggle_capture(_app: &AppHandle) {}

#[cfg(not(desktop))]
#[allow(dead_code)]
pub fn create_capture_window(_app: &AppHandle) -> tauri::Result<()> {
    Ok(())
}
