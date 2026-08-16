//! Tray menu: Show / Hide / update entry / Open Log Directory / Quit.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

use crate::logs::open_logs_dir;
use crate::paths::default_dsh_home;
use crate::update::{UpdateState, UpdateStatus};
use crate::AppState;

/// The update row, kept so its label can follow the check result. The tray menu
/// is built once; only this item's text changes afterwards.
static UPDATE_ITEM: OnceLock<Mutex<Option<MenuItem<tauri::Wry>>>> = OnceLock::new();

/// Label for the current update status.
fn update_label(status: &UpdateStatus) -> String {
    match status {
        UpdateStatus::Unknown => "Check for Updates".into(),
        UpdateStatus::UpToDate => "No Updates Available".into(),
        UpdateStatus::Available { version } => format!("Update to {version}"),
        UpdateStatus::Installing => "Installing Update…".into(),
    }
}

/// Re-render the update row from the live status.
///
/// Safe to call from any thread and before the tray exists — a missing item is
/// simply skipped, which is what happens if a check completes during teardown.
///
/// # Parameters
/// - `app`: Tauri app handle.
pub fn refresh_update_item(app: &AppHandle) {
    let Some(cell) = UPDATE_ITEM.get() else {
        return;
    };
    let guard = cell.lock().expect("tray update item mutex");
    let Some(item) = guard.as_ref() else { return };
    let Some(state) = app.try_state::<Arc<UpdateState>>() else {
        return;
    };
    let status = state.status();
    let _ = item.set_text(update_label(&status));
    // Only an offered update is actionable; the other states are informational.
    let _ = item.set_enabled(matches!(
        status,
        UpdateStatus::Unknown | UpdateStatus::Available { .. }
    ));
}

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
    let open_browser =
        MenuItem::with_id(app, "open-in-browser", "在浏览器中打开", true, None::<&str>)?;
    let share = MenuItem::with_id(
        app,
        "open-share-window",
        "在其他设备上使用…",
        true,
        None::<&str>,
    )?;
    let logs = MenuItem::with_id(app, "logs", "Open Log Directory", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let update = MenuItem::with_id(
        app,
        "update",
        update_label(&UpdateStatus::Unknown),
        true,
        None::<&str>,
    )?;
    let separator = PredefinedMenuItem::separator(app)?;
    let separator2 = PredefinedMenuItem::separator(app)?;
    let separator3 = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(
        app,
        &[
            &show,
            &hide,
            &separator,
            &open_browser,
            &share,
            &separator2,
            &update,
            &separator3,
            &logs,
            &quit,
        ],
    )?;
    let _ = UPDATE_ITEM.set(Mutex::new(Some(update)));
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
            "open-in-browser" | "open-share-window" => {
                let _ = crate::share::dispatch_menu(app, event.id.as_ref());
            }
            // The window is on the sidecar's origin once boot finishes, so the
            // start page's button is gone: this is the only entry to the logs
            // for a session that misbehaves after it started.
            "logs" => {
                if let Err(err) = open_logs_dir(&default_dsh_home()) {
                    eprintln!("desktop: failed to open the log directory: {err}");
                }
            }
            // Unknown means no check has landed yet (offline, or still inside
            // the startup delay), so the click runs one on demand.
            "update" => {
                let handle = app.clone();
                tauri::async_runtime::spawn(async move {
                    let status = handle
                        .try_state::<Arc<UpdateState>>()
                        .map(|state| state.status());
                    match status {
                        Some(UpdateStatus::Available { .. }) => {
                            crate::update::install_and_restart(handle).await
                        }
                        Some(_) => crate::update::check(&handle).await,
                        None => {}
                    }
                });
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
