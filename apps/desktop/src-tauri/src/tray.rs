//! Menu-bar (tray) icon: a native template image with a control menu.
//!
//! macOS convention: clicking a menu-bar extra opens its menu (both mouse
//! buttons open it here). The menu mirrors the app: live status, a
//! connect/disconnect toggle, "show window", settings and quit. Closing the
//! window hides the app here instead of quitting; "Quit" and Cmd+Q go
//! through the graceful shutdown in `lib.rs`.
//!
//! The icon is a template image (shield with a keyhole), so macOS renders
//! it in the menu-bar colour and adapts to light/dark themes automatically.

use std::time::Duration;

use tauri::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Emitter, Manager, Wry};

use crate::AppState;

/// Managed once the tray exists; `lib.rs` uses its presence to decide
/// whether closing the window should hide (tray) or quit (no tray).
pub struct Availability;

const TRAY_ID: &str = "kvn-tray";

const ID_STATUS: &str = "kvn.status";
const ID_TOGGLE: &str = "kvn.toggle";
const ID_SHOW: &str = "kvn.show";
const ID_SETTINGS: &str = "kvn.settings";
const ID_QUIT: &str = "kvn.quit";

/// Minimal two-language strings for the tray (the windowed UI localizes in
/// the frontend; the tray lives in Rust).
fn phrase(lang: &str, key: &str) -> &'static str {
    match (lang, key) {
        ("en", "connecting") => "Connecting…",
        ("en", "reconnecting") => "Reconnecting…",
        ("en", "stopping") => "Disconnecting…",
        ("en", "connected") => "Connected",
        ("en", "error") => "Connection failed",
        ("en", "disconnected") => "Disconnected",
        ("en", "connect") => "Connect",
        ("en", "disconnect") => "Disconnect",
        ("en", "show") => "Show KVN",
        ("en", "settings") => "Settings…",
        ("en", "quit") => "Quit KVN",
        (_, "connecting") => "Подключение…",
        (_, "reconnecting") => "Переподключение…",
        (_, "stopping") => "Отключение…",
        (_, "connected") => "Подключено",
        (_, "error") => "Ошибка подключения",
        (_, "disconnected") => "Не подключено",
        (_, "connect") => "Подключиться",
        (_, "disconnect") => "Отключиться",
        (_, "show") => "Показать KVN",
        (_, "settings") => "Настройки…",
        (_, "quit") => "Завершить KVN",
        (_, _) => "KVN",
    }
}

/// Build the tray icon and wire its menu. Call after `AppState` is managed.
pub fn init(app: &AppHandle) -> tauri::Result<()> {
    let lang = language(app);
    let status = MenuItem::with_id(
        app,
        ID_STATUS,
        phrase(&lang, "disconnected"),
        false,
        None::<&str>,
    )?;
    let toggle = MenuItem::with_id(app, ID_TOGGLE, phrase(&lang, "connect"), true, None::<&str>)?;
    let show = MenuItem::with_id(app, ID_SHOW, phrase(&lang, "show"), true, None::<&str>)?;
    let settings = MenuItem::with_id(
        app,
        ID_SETTINGS,
        phrase(&lang, "settings"),
        true,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, ID_QUIT, phrase(&lang, "quit"), true, None::<&str>)?;

    let menu = Menu::with_items(
        app,
        &[
            &status,
            &PredefinedMenuItem::separator(app)?,
            &toggle,
            &PredefinedMenuItem::separator(app)?,
            &show,
            &settings,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;

    let tray = TrayIconBuilder::with_id(TRAY_ID)
        .icon(tauri::include_image!("icons/tray-template.png"))
        .icon_as_template(true)
        .tooltip("KVN")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(on_menu_event)
        .build(app)?;

    spawn_status_updater(app.clone(), status, toggle, tray);
    Ok(())
}

fn language(app: &AppHandle) -> String {
    app.try_state::<AppState>()
        .map(|state| state.store.snapshot().settings.language)
        .unwrap_or_else(|| "ru".to_owned())
}

fn on_menu_event(app: &AppHandle, event: MenuEvent) {
    match event.id().as_ref() {
        // The frontend owns the connect/disconnect decision (it mirrors the
        // main button: cancel while connecting, disconnect when up).
        ID_TOGGLE => emit_to_window(app, "tray-toggle"),
        ID_SHOW => show_window(app),
        ID_SETTINGS => {
            show_window(app);
            emit_to_window(app, "open-settings");
        }
        ID_QUIT => app.exit(0),
        _ => {}
    }
}

fn emit_to_window(app: &AppHandle, event: &str) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.emit(event, ());
    }
}

fn show_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Keep the status line, the toggle label, the tooltip and the app-language
/// switches in sync; one cheap pass per second.
fn spawn_status_updater(
    app: AppHandle,
    status_item: MenuItem<Wry>,
    toggle_item: MenuItem<Wry>,
    tray: TrayIcon<Wry>,
) {
    tauri::async_runtime::spawn(async move {
        let mut last: Option<(String, String)> = None;
        loop {
            tokio::time::sleep(Duration::from_millis(1000)).await;
            let Some(state) = app.try_state::<AppState>() else {
                return;
            };
            let data = state.store.snapshot();
            let lang = data.settings.language.clone();
            let phase = state.shared.state.read().await.clone();
            let profile = data
                .active_profile
                .as_deref()
                .and_then(|id| data.profile(id))
                .map(|p| p.name.clone());

            let status_text = match &phase {
                kvn_core::state::ConnectionState::Disconnected => {
                    phrase(&lang, "disconnected").to_owned()
                }
                kvn_core::state::ConnectionState::Connecting { .. } => {
                    phrase(&lang, "connecting").to_owned()
                }
                kvn_core::state::ConnectionState::Connected { .. } => match &profile {
                    Some(name) => format!("{} · {name}", phrase(&lang, "connected")),
                    None => phrase(&lang, "connected").to_owned(),
                },
                kvn_core::state::ConnectionState::Reconnecting { .. } => {
                    phrase(&lang, "reconnecting").to_owned()
                }
                kvn_core::state::ConnectionState::Stopping { .. } => {
                    phrase(&lang, "stopping").to_owned()
                }
                kvn_core::state::ConnectionState::Error { .. } => phrase(&lang, "error").to_owned(),
            };
            let toggle_text = match &phase {
                kvn_core::state::ConnectionState::Connecting { .. }
                | kvn_core::state::ConnectionState::Connected { .. }
                | kvn_core::state::ConnectionState::Reconnecting { .. } => {
                    phrase(&lang, "disconnect").to_owned()
                }
                _ => phrase(&lang, "connect").to_owned(),
            };

            if last.as_ref() != Some(&(status_text.clone(), toggle_text.clone())) {
                let _ = status_item.set_text(&status_text);
                let _ = toggle_item.set_text(&toggle_text);
                let _ = tray.set_tooltip(Some(&format!("KVN — {status_text}")));
                last = Some((status_text, toggle_text));
            }
        }
    });
}
