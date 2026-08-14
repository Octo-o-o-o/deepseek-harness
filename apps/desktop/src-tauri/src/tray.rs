//! Tray menu: Show / Hide / Open Log Directory / Quit.

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

use crate::logs::open_logs_dir;
use crate::paths::default_dsh_home;
use crate::AppState;

/// Install the status-item menu and left-click-to-show behavior.
///
/// # Parameters
/// - `app`: Tauri app handle.
///
/// # Returns
/// `Ok(())` after the tray icon is registered.
pub fn install_tray(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Show", true, None::<&str>)?;
    let hide = MenuItem::with_id(app, "hide", "Hide", true, None::<&str>)?;
    let logs = MenuItem::with_id(app, "logs", "Open Log Directory", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&show, &hide, &separator, &logs, &quit])?;
    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| std::io::Error::other("desktop: missing window icon"))?;

    TrayIconBuilder::new()
        .icon(icon)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main(app),
            "hide" => hide_main(app),
            // The window is on the sidecar's origin once boot finishes, so the
            // start page's button is gone: this is the only entry to the logs
            // for a session that misbehaves after it started.
            "logs" => {
                if let Err(err) = open_logs_dir(&default_dsh_home()) {
                    eprintln!("desktop: failed to open the log directory: {err}");
                }
            }
            "quit" => quit_app(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

/// Show, unminimize, and focus the main window.
///
/// # Parameters
/// - `app`: Tauri app handle.
pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn hide_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
}

fn quit_app(app: &AppHandle) {
    if let Some(state) = app.try_state::<AppState>() {
        state.request_stop();
    }
    app.exit(0);
}
