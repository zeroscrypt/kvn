//! # KVN desktop application
//!
//! Tauri shell around the connection [`controller`] and the UI-facing
//! [`commands`]. All domain logic lives in `kvn-core` (parsing, config
//! generation, state machine) and `kvn-engine` (process management,
//! health checks, stats, verified install); this crate wires them to the
//! window and persists state under the app data directory.

mod commands;
mod controller;
mod engine_install;
mod eventlog;
mod paths;
mod store;
mod sysproxy;
mod tray;

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use tauri::Manager;

pub use commands::AppState;
pub use controller::{Controller, Shared};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .setup(|app| {
            let handle = app.handle().clone();
            let paths = paths::AppPaths::resolve(&handle)
                .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
            let log = Arc::new(eventlog::EventLog::with_file(
                2000,
                paths.logs_dir.join("kvn.log"),
            ));
            let store = Arc::new(store::Store::load(paths.state_file.clone(), &log));
            log.push(
                "info",
                "app",
                format!("kvn {} starting", env!("CARGO_PKG_VERSION")),
            );

            // Crash recovery: a leftover proxy backup means the previous run
            // died while the system proxy pointed at our (now dead) port.
            sysproxy::restore_backup_if_present(&paths, &log);

            let shared = Arc::new(controller::Shared::new());
            let controller = controller::Controller::start(
                Arc::clone(&store),
                Arc::clone(&log),
                Arc::clone(&shared),
                paths.clone(),
            );

            app.manage(AppState {
                paths,
                store,
                log,
                shared,
                controller,
                installing: AtomicBool::new(false),
                ui_seen: AtomicBool::new(false),
            });

            // Menu-bar icon: with it, closing the window hides the app to
            // the tray instead of quitting (see `on_window_event` below).
            if let Err(e) = tray::init(&handle) {
                let state = app.state::<AppState>();
                state.log.push(
                    "warn",
                    "tray",
                    format!("menu-bar icon unavailable ({e}); closing the window will quit"),
                );
            } else {
                app.manage(tray::Availability);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_status,
            commands::list_profiles,
            commands::import_from_text,
            commands::delete_profile,
            commands::rename_profile,
            commands::set_active_profile,
            commands::connect,
            commands::disconnect,
            commands::ping_profile,
            commands::get_events,
            commands::clear_events,
            commands::subscription_list,
            commands::subscription_add,
            commands::subscription_update,
            commands::subscription_delete,
            commands::settings_get,
            commands::settings_set,
            commands::engine_status,
            commands::engine_install,
            commands::engine_set_path,
            commands::engine_check_update,
            commands::system_proxy_set,
            commands::diagnostics_export,
            commands::ui_report,
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let tray_running = window
                    .app_handle()
                    .try_state::<tray::Availability>()
                    .is_some();
                if tray_running {
                    // Keep living in the menu bar; quitting is explicit
                    // (tray menu or Cmd+Q) and runs the graceful shutdown.
                    api.prevent_close();
                    if let Some(state) = window.app_handle().try_state::<AppState>() {
                        state
                            .log
                            .push("info", "app", "window hidden to the menu bar");
                    }
                    let _ = window.hide();
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building the KVN window");

    app.run(|app_handle, event| {
        if let tauri::RunEvent::ExitRequested { .. } = event {
            let Some(state) = app_handle.try_state::<AppState>() else {
                return;
            };
            let shared = Arc::clone(&state.shared);
            let controller = state.controller.clone();
            let paths = state.paths.clone();
            let log = Arc::clone(&state.log);
            tauri::async_runtime::block_on(async move {
                // Graceful: ask the state machine to stop, then make sure no
                // engine process survives the app either way.
                controller.stop();
                for _ in 0..20 {
                    if matches!(
                        *shared.state.read().await,
                        kvn_core::state::ConnectionState::Disconnected
                    ) {
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
                controller.shutdown_engine().await;
                if shared.sysproxy_on.load(std::sync::atomic::Ordering::SeqCst) {
                    match controller::disable_sysproxy(&paths, &log).await {
                        Ok(()) => {
                            log.push("info", "sysproxy", "system proxy restored on exit");
                        }
                        Err(e) => {
                            log.push("warn", "sysproxy", e);
                        }
                    }
                }
            });
        }
    });
}
