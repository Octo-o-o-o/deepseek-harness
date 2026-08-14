//! DeepSeek Harness desktop shell.
//!
//! Tauri is a thin host. Sidecar spawn, ready-line parsing, health checks,
//! process-tree shutdown, and bind safety live in injectable modules.

mod env;
mod health;
mod http;
#[cfg(windows)]
mod job;
mod lock;
mod logs;
mod migrate;
mod navigation;
mod opener;
mod overlay;
mod paths;
mod pid;
mod process;
mod ready;
mod shell_env;
mod sidecar;
mod state;
mod supervisor;
mod token;
mod tray;

use std::path::Path;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use tauri::webview::NewWindowResponse;
use tauri::{AppHandle, Manager, WebviewWindowBuilder};

use crate::health::{check_host_described, check_loader_ready, wait_desktop_client_ready};
use crate::lock::{try_lock_home, HomeLock};
use crate::logs::{install_panic_hook, open_logs_dir, rotate_sidecar_log};
use crate::migrate::{
    default_legacy_home, inject_fault_from_env, migrate_legacy_home, MigrationReport,
};
use crate::navigation::is_internal_url;
use crate::opener::open_external_url;
use crate::paths::{
    default_dsh_home, default_workspace_cwd, ensure_desktop_state, resolve_node, resolve_web_bin,
};
use crate::pid::{clear_sidecar_pid, reap_stale_sidecar, write_sidecar_pid};
use crate::shell_env::login_shell_env;
use crate::sidecar::{desktop_web_args, spawn_sidecar, wait_ready, SidecarSpec};
use crate::state::{transition, BootEvent, BootPhase};
use crate::supervisor::SidecarSupervisor;
use crate::token::generate_desktop_token;
use crate::tray::{install_tray, show_main};

/// Shared runtime state held by the Tauri app.
pub struct AppState {
    supervisor: SidecarSupervisor,
    boot: Mutex<Option<thread::JoinHandle<()>>>,
    home: Mutex<Option<std::path::PathBuf>>,
    _lock: Mutex<Option<HomeLock>>,
}

impl AppState {
    /// Unique stop entry: cancel boot, take the sidecar out of the lock, shut it down.
    pub fn request_stop(&self) {
        self.supervisor.request_stop();
        if let Ok(home) = self.home.lock() {
            if let Some(home) = home.as_ref() {
                clear_sidecar_pid(home);
            }
        }
    }
}

/// Ready-line wait used by the supervisor.
const READY_TIMEOUT: Duration = Duration::from_secs(15);

/// Launch the desktop shell.
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .manage(AppState {
            supervisor: SidecarSupervisor::new(),
            boot: Mutex::new(None),
            home: Mutex::new(None),
            _lock: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![open_log_directory])
        .setup(|app| {
            install_tray(app.handle())?;
            let signal_handle = app.handle().clone();
            if let Err(err) = ctrlc::set_handler(move || {
                if let Some(state) = signal_handle.try_state::<AppState>() {
                    state.request_stop();
                }
                signal_handle.exit(0);
            }) {
                eprintln!("desktop: failed to install shutdown handler: {err}");
            }
            let window = build_main_window(app.handle())?;
            let handle = app.handle().clone();
            let boot = thread::spawn(move || {
                let state = handle.state::<AppState>();
                let exe = std::env::current_exe().unwrap_or_else(|_| Path::new(".").to_path_buf());
                let cwd = std::env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf());
                if let Err(message) = boot_and_navigate(&window, &state, &exe, &cwd) {
                    state.request_stop();
                    show_error(&window, &message);
                }
            });
            if let Some(state) = app.try_state::<AppState>() {
                if let Ok(mut guard) = state.boot.lock() {
                    *guard = Some(boot);
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .build(tauri::generate_context!())
        .expect("failed to build dshd")
        .run(|app, event| match event {
            // Stop only. Joining the boot thread here deadlocks: this runs on
            // the main thread, and the boot thread's calls into the WebView are
            // answered by this same loop.
            tauri::RunEvent::Exit => {
                if let Some(state) = app.try_state::<AppState>() {
                    state.request_stop();
                }
            }
            // Closing the window hides it, so the Dock icon is the only
            // affordance left; macOS reports that click here and nowhere else.
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Reopen {
                has_visible_windows,
                ..
            } => {
                if !has_visible_windows {
                    show_main(app);
                }
            }
            // `RunEvent` is `#[non_exhaustive]`: a Tauri upgrade may add
            // variants this shell has no behavior for.
            _ => {}
        });
}

/// Create the main window from its `tauri.conf.json` entry, adding the
/// navigation rules a configured window cannot carry.
///
/// The window is declared with `"create": false` so this builder owns it: the
/// navigation and new-window handlers exist only on the builder, and without
/// them a link in page content would either replace the whole application UI
/// or, for `target="_blank"`, do nothing at all.
///
/// # Parameters
/// - `app`: Tauri app handle.
///
/// # Returns
/// The built main window.
fn build_main_window(app: &AppHandle) -> tauri::Result<tauri::WebviewWindow> {
    let config = app
        .config()
        .app
        .windows
        .first()
        .cloned()
        .ok_or_else(|| tauri::Error::WindowNotFound)?;
    WebviewWindowBuilder::from_config(app, &config)?
        .on_navigation(|url| {
            if is_internal_url(url) {
                return true;
            }
            let _ = open_external_url(url);
            false
        })
        .on_new_window(|url, _features| {
            let _ = open_external_url(&url);
            NewWindowResponse::Deny
        })
        .build()
}

fn boot_and_navigate(
    window: &tauri::WebviewWindow,
    state: &AppState,
    exe: &Path,
    cwd: &Path,
) -> Result<(), String> {
    let node = resolve_node(exe).map_err(|err| err.to_string())?;
    let bin = resolve_web_bin(exe, cwd).map_err(|err| err.to_string())?;
    let home = default_dsh_home();
    std::fs::create_dir_all(home.join("logs")).map_err(|err| err.to_string())?;
    install_panic_hook(home.clone());
    rotate_sidecar_log(&home.join("logs/sidecar.log")).map_err(|err| err.to_string())?;
    let held = try_lock_home(&home).map_err(|err| err.to_string())?;
    {
        let mut guard = state._lock.lock().map_err(|err| err.to_string())?;
        *guard = Some(held);
    }
    {
        let mut guard = state.home.lock().map_err(|err| err.to_string())?;
        *guard = Some(home.clone());
    }
    reap_stale_sidecar(&home);
    let report = migrate_legacy_home(&default_legacy_home(), &home, inject_fault_from_env())
        .map_err(|err| err.to_string())?;
    if report.migrated {
        show_migration(window, &report);
    }
    let workspace = default_workspace_cwd(&home);
    ensure_desktop_state(&home, &workspace).map_err(|err| err.to_string())?;
    let token = generate_desktop_token().map_err(|err| err.to_string())?;
    let nonce = generate_desktop_token().map_err(|err| err.to_string())?;
    let mut env = vec![
        ("DSH_HOME".into(), home.to_string_lossy().into_owned()),
        ("DSH_DESKTOP_TOKEN".into(), token.clone()),
        ("DSH_DESKTOP_BOOTSTRAP_NONCE".into(), nonce.clone()),
    ];
    if let Some(node_modules) = bin
        .parent()
        .and_then(|lib| lib.parent())
        .map(|app| app.join("node_modules"))
    {
        if node_modules.is_dir() {
            env.push((
                "NODE_PATH".into(),
                node_modules.to_string_lossy().into_owned(),
            ));
        }
    }
    let spec = SidecarSpec {
        program: node,
        args: desktop_web_args(&bin, &[]),
        cwd: workspace,
        env,
        login_env: login_shell_env(),
        log_path: home.join("logs/sidecar.log"),
    };
    let mut phase = BootPhase::Idle;
    let spawned = spawn_sidecar(&spec).map_err(|err| err.to_string())?;
    phase = transition(phase, BootEvent::SpawnOk);
    let (process, ready) = spawned.into_parts();
    write_sidecar_pid(&home, process.pid(), &bin);
    state.supervisor.install(process);
    if state.supervisor.is_cancelled() {
        state.request_stop();
        return Err("boot cancelled".into());
    }
    let port = match wait_ready(&ready, READY_TIMEOUT) {
        Ok(port) => port,
        Err(err) => {
            state.request_stop();
            let _ = transition(
                phase,
                BootEvent::Failed {
                    reason: err.to_string(),
                },
            );
            return Err(err.to_string());
        }
    };
    phase = transition(phase, BootEvent::Bound { port });
    if state.supervisor.is_cancelled() {
        state.request_stop();
        return Err("boot cancelled".into());
    }
    if let Err(err) = check_loader_ready(port) {
        state.request_stop();
        let _ = transition(
            phase,
            BootEvent::Failed {
                reason: err.to_string(),
            },
        );
        return Err(err.to_string());
    }
    phase = transition(phase, BootEvent::LoaderReady);
    if let Err(err) = check_host_described(port, &token) {
        state.request_stop();
        let _ = transition(
            phase,
            BootEvent::Failed {
                reason: err.to_string(),
            },
        );
        return Err(err.to_string());
    }
    phase = transition(phase, BootEvent::HostDescribed);
    if let Err(err) = navigate_to_sidecar(window, port) {
        state.request_stop();
        return Err(err);
    }
    if let Err(err) = wait_desktop_client_ready(port, &nonce, READY_TIMEOUT) {
        state.request_stop();
        let _ = transition(
            phase,
            BootEvent::Failed {
                reason: err.to_string(),
            },
        );
        return Err(err.to_string());
    }
    phase = transition(phase, BootEvent::WsReady);
    let _ = transition(phase, BootEvent::Visible);
    if state.supervisor.wait_for_unexpected_exit(SIDECAR_POLL) {
        return Err("the local host stopped unexpectedly".into());
    }
    Ok(())
}

/// How often the boot thread asks whether the sidecar is still running.
const SIDECAR_POLL: Duration = Duration::from_secs(2);

#[tauri::command]
fn open_log_directory() -> Result<(), String> {
    open_logs_dir(&default_dsh_home()).map_err(|err| err.to_string())
}

fn show_migration(window: &tauri::WebviewWindow, report: &MigrationReport) {
    let summary = format!(
        "Copied {}.\nSkipped credentials.\nBackup: {}",
        if report.copied.is_empty() {
            "(nothing)".into()
        } else {
            report.copied.join(", ")
        },
        report
            .backup
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "(none)".into()),
    );
    let encoded = serde_json::to_string(&summary).unwrap_or_else(|_| "\"migrated\"".into());
    let script =
        format!("window.__DSH_SHOW_MIGRATION__ && window.__DSH_SHOW_MIGRATION__({encoded})");
    let _ = window.eval(&script);
}

fn show_error(window: &tauri::WebviewWindow, message: &str) {
    // The window may be on the sidecar's origin, which has no error page and is
    // in any case the thing that just failed. The bundled start page owns the
    // message and the button that opens the log directory.
    if window.url().is_ok_and(|url| url.scheme() == "http") {
        if let Ok(start) = tauri::Url::parse("tauri://localhost/") {
            let _ = window.navigate(start);
            thread::sleep(Duration::from_millis(300));
        }
    }
    let _ = window.show();
    let _ = window.set_focus();
    let encoded = serde_json::to_string(message).unwrap_or_else(|_| "\"boot failed\"".into());
    let script = format!("window.__DSH_SHOW_ERROR__ && window.__DSH_SHOW_ERROR__({encoded})");
    let _ = window.eval(&script);
}

/// Whether `url` is served by the sidecar this launch spawned.
///
/// The port is compared as a number rather than as the text of the URL: the
/// prefix of `http://127.0.0.1:1234` is also a prefix of `http://127.0.0.1:12345`.
///
/// # Parameters
/// - `url`: the WebView's current URL.
/// - `port`: loopback port from the ready line.
///
/// # Returns
/// `true` when scheme, host, and port all match the running sidecar.
fn is_sidecar_origin(url: &tauri::Url, port: u16) -> bool {
    url.scheme() == "http" && url.host_str() == Some("127.0.0.1") && url.port() == Some(port)
}

fn navigate_to_sidecar(window: &tauri::WebviewWindow, port: u16) -> Result<(), String> {
    let url = format!("http://127.0.0.1:{port}/");
    let parsed = tauri::Url::parse(&url).map_err(|err| err.to_string())?;
    window.navigate(parsed).map_err(|err| err.to_string())?;
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    while std::time::Instant::now() < deadline {
        if let Ok(current) = window.url() {
            if is_sidecar_origin(&current, port) {
                return Ok(());
            }
        }
        thread::sleep(Duration::from_millis(50));
    }
    Err("webview navigation did not finish".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(raw: &str) -> tauri::Url {
        tauri::Url::parse(raw).expect("test url")
    }

    #[test]
    fn sidecar_origin_matches_scheme_host_and_port() {
        assert!(is_sidecar_origin(&url("http://127.0.0.1:1234/"), 1234));
        assert!(is_sidecar_origin(
            &url("http://127.0.0.1:1234/session/1"),
            1234
        ));
    }

    #[test]
    fn a_longer_port_is_not_the_sidecar() {
        assert!(!is_sidecar_origin(&url("http://127.0.0.1:12345/"), 1234));
        assert!(!is_sidecar_origin(&url("https://127.0.0.1:1234/"), 1234));
        assert!(!is_sidecar_origin(&url("http://localhost:1234/"), 1234));
    }
}
