//! Window helpers: the capture palette and the main window.

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

pub const MAIN: &str = "main";
pub const CAPTURE: &str = "capture";

pub fn show_capture(app: &AppHandle) {
    let Some(window) = app.get_webview_window(CAPTURE) else {
        return;
    };
    let _ = window.show();
    let _ = window.set_focus();
    let _ = window.center();
}

pub fn hide_capture(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(CAPTURE) {
        let _ = window.hide();
    }
}

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

pub fn create_capture_window(app: &AppHandle) -> tauri::Result<()> {
    let window =
        WebviewWindowBuilder::new(app, CAPTURE, WebviewUrl::App("index.html#/capture".into()))
            .title("MoneyTracker Capture")
            .inner_size(640.0, 148.0)
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

pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN) {
        let _ = window.show();
        let _ = window.set_focus();
    }
}
