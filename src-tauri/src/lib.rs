#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod commands;
mod editing;
mod shortcut;
mod state;
mod watcher;
mod window;

use std::sync::Arc;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};
use tauri_plugin_positioner::{Position, WindowExt};
use tauri_specta::{collect_commands, collect_events, Builder};

use crate::state::AppState;

pub fn build_specta() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new()
        .commands(collect_commands![
            commands::vault_create,
            commands::vault_open,
            commands::vault_close,
            commands::vault_recent,
            commands::vault_info,
            commands::ledger_snapshot,
            commands::ledger_query,
            commands::dashboard,
            commands::reload_vault,
            commands::capture_parse,
            commands::capture_show,
            commands::capture_hide,
            commands::capture_commit,
            commands::txn_get,
            commands::txn_update,
            commands::txn_delete,
            commands::txn_bulk_update,
            commands::rule_list,
            commands::rule_upsert,
            commands::rule_toggle,
            commands::rule_test,
            commands::rule_recompute,
            commands::accounts_tree,
            commands::account_upsert,
            commands::budget_list,
            commands::forecast,
            commands::fx_missing,
            commands::fx_apply_rates,
            commands::write_audit,
            commands::settings_get,
            commands::settings_set,
            commands::shortcut_status,
            commands::sample_vault_path,
            commands::create_sample_vault,
            commands::app_version,
        ])
        .events(collect_events![
            commands::LedgerChanged,
            commands::CaptureSaved,
            commands::ParseDiagnostics,
        ])
}

fn setup_tray(app: &tauri::App) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "打开 MoneyTracker", true, None::<&str>)?;
    let capture = MenuItem::with_id(app, "capture", "快速记账", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&capture, &show, &separator, &quit])?;

    let mut builder = TrayIconBuilder::with_id("moneytracker")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip("MoneyTracker — 快速记账")
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => window::show_main(app),
            "capture" => window::show_capture(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                if let Some(win) = app.get_webview_window(window::CAPTURE) {
                    let _ = win.as_ref().window().move_window(Position::TrayCenter);
                }
                window::toggle_capture(app);
            }
        });

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

fn boot(app: &AppHandle) {
    let state = app.state::<AppState>();
    let settings = state.settings();
    if let Some(path) = settings.vault_path.clone() {
        let opts = settings.vault_options();
        if let Ok(vault) = ledger_engine::Vault::load(std::path::Path::new(&path), &opts) {
            state.set_vault(Some(Arc::new(vault)));
            watcher::watch(app.clone(), std::path::Path::new(&path).to_path_buf());
        }
    }
    if let Err(err) = shortcut::register(app, &settings.shortcut) {
        eprintln!("moneytracker: {err}");
        state.set_shortcut_error(Some(err));
    }
}

pub fn run() {
    let specta = build_specta();

    #[cfg(debug_assertions)]
    specta
        .export(
            specta_typescript::Typescript::default(),
            "../src/lib/bindings.ts",
        )
        .expect("failed to export typescript bindings");

    let specta_for_setup = specta;

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_positioner::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if event.state() == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                        let settings = app.state::<AppState>().settings();
                        if let Ok(expected) =
                            <tauri_plugin_global_shortcut::Shortcut as std::str::FromStr>::from_str(
                                &settings.shortcut,
                            )
                        {
                            if expected == *shortcut {
                                window::toggle_capture(app);
                            }
                        }
                    }
                })
                .build(),
        )
        .manage(AppState::new(state::default_config_dir()))
        .invoke_handler(specta_for_setup.invoke_handler())
        .setup(move |app| {
            specta_for_setup.mount_events(app);
            let handle = app.handle().clone();
            setup_tray(app)?;
            if app.get_webview_window(window::CAPTURE).is_none() {
                let _ = window::create_capture_window(&handle);
            }
            boot(&handle);
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == window::CAPTURE {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running MoneyTracker");
}
