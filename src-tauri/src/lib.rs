//! Tauri shell: wires the pure-Rust core to the WebView UI, the tray icon and
//! the Windows integrations.

mod commands;
mod tray;

use commands::AppState;
use networld_core::manager::ManagerEvent;
use networld_core::{Paths, VpnManager};
use std::sync::Arc;
use tauri::{Emitter, Manager};

pub const EVT_STATUS: &str = "networld://status";
pub const EVT_STATS: &str = "networld://stats";
pub const EVT_LOG: &str = "networld://log";
pub const EVT_CONFIG: &str = "networld://config-changed";

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    networld_core::init();

    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init());

    #[cfg(desktop)]
    {
        builder = builder
            .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.unminimize();
                    let _ = w.set_focus();
                }
            }))
            .plugin(tauri_plugin_autostart::init(
                tauri_plugin_autostart::MacosLauncher::LaunchAgent,
                Some(vec!["--minimized"]),
            ));
    }

    builder
        .setup(|app| {
            let manager = VpnManager::new(Paths::discover());
            app.manage(AppState {
                manager: manager.clone(),
            });

            tray::build(app.handle(), manager.clone())?;
            spawn_event_bridge(app.handle().clone(), manager.clone());
            spawn_autoconnect(app.handle().clone(), manager);

            // Launching with --minimized (autostart) keeps the window hidden.
            if std::env::args().any(|a| a == "--minimized") {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.hide();
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the window parks the app in the tray instead of quitting,
            // so an active tunnel keeps running.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::get_status,
            commands::get_config,
            commands::update_settings,
            commands::list_profiles,
            commands::save_profile,
            commands::lock_profile,
            commands::delete_profile,
            commands::duplicate_profile,
            commands::set_active_profile,
            commands::test_profile,
            commands::connect,
            commands::disconnect,
            commands::import_links,
            commands::import_nw,
            commands::export_nw,
            commands::profile_to_link,
            commands::payload_presets,
            commands::preview_payload,
            commands::save_payload,
            commands::delete_payload,
            commands::get_logs,
            commands::clear_logs,
            commands::export_logs,
            commands::speed_test,
            commands::get_data_usage,
            commands::open_config_dir,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Networld VPN");
}

/// Forward core events (status, stats, log lines) to the WebView.
fn spawn_event_bridge(app: tauri::AppHandle, manager: Arc<VpnManager>) {
    let mut events = manager.subscribe();
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        while let Ok(evt) = events.recv().await {
            match evt {
                ManagerEvent::Status(s) => {
                    let _ = handle.emit(EVT_STATUS, s);
                }
                ManagerEvent::Stats(s) => {
                    let _ = handle.emit(EVT_STATS, s);
                }
                ManagerEvent::ConfigChanged => {
                    let _ = handle.emit(EVT_CONFIG, ());
                }
            }
        }
    });

    let mut logs = manager.logs.subscribe();
    tauri::async_runtime::spawn(async move {
        while let Ok(entry) = logs.recv().await {
            let _ = app.emit(EVT_LOG, entry);
        }
    });
}

/// Honour "connect on launch".
fn spawn_autoconnect(_app: tauri::AppHandle, manager: Arc<VpnManager>) {
    tauri::async_runtime::spawn(async move {
        let settings = manager.settings().await;
        if settings.connect_on_launch {
            tokio::time::sleep(std::time::Duration::from_millis(600)).await;
            let _ = manager.connect(None).await;
        }
    });
}
