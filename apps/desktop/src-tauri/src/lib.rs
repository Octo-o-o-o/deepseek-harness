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
mod update;

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
use crate::tray::install_tray;
#[cfg(target_os = "macos")]
use crate::tray::show_main;

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
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .manage(std::sync::Arc::new(update::UpdateState::new()))
        .manage(AppState {
            supervisor: SidecarSupervisor::new(),
            boot: Mutex::new(None),
            home: Mutex::new(None),
            _lock: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![open_log_directory])
        .setup(|app| {
            install_tray(app.handle())?;
            update::spawn_checker(app.handle().clone());
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
                let exe = std::env::current_exe().unwrap_or_else(|_| Path::new(".").to_path_buf());
                let cwd = std::env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf());
                let splash = splash_url(&window);
                if let Err(message) =
                    boot_and_navigate(&handle, &window, &exe, &cwd, splash.as_ref())
                {
                    eprintln!("desktop: boot failed: {message}");
                    handle.state::<AppState>().request_stop();
                    show_error(&window, splash.as_ref(), &message);
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
    handle: &tauri::AppHandle,
    window: &tauri::WebviewWindow,
    exe: &Path,
    cwd: &Path,
    splash: Option<&tauri::Url>,
) -> Result<(), String> {
    let state = handle.state::<AppState>();
    let node = resolve_node(exe).map_err(|err| err.to_string())?;
    let bin = resolve_web_bin(exe, cwd).map_err(|err| err.to_string())?;
    let home = default_dsh_home();
    std::fs::create_dir_all(home.join("logs")).map_err(|err| err.to_string())?;
    install_panic_hook(home.clone());
    let held = try_lock_home(&home).map_err(|err| err.to_string())?;
    {
        let mut guard = state._lock.lock().map_err(|err| err.to_string())?;
        *guard = Some(held);
    }
    {
        let mut guard = state.home.lock().map_err(|err| err.to_string())?;
        *guard = Some(home.clone());
    }
    // Rotate only after the lock is held: a second instance must not rotate
    // the first instance's live log before it is turned away.
    rotate_sidecar_log(&home.join("logs/sidecar.log")).map_err(|err| err.to_string())?;
    reap_stale_sidecar(&home);
    let report = migrate_legacy_home(&default_legacy_home(), &home, inject_fault_from_env())
        .map_err(|err| err.to_string())?;
    if report.migrated {
        show_migration(window, splash, &report);
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
    write_sidecar_pid(&home, process.pid(), &bin, process.start_token());
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
    if let Err(err) = navigate_to_sidecar(window, port, &nonce) {
        state.request_stop();
        return Err(err);
    }
    if inject_boot_fault("client-ready") {
        state.request_stop();
        return Err("injected fault: desktop client ready wait".into());
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

/// Test injection point named by `DSH_DESKTOP_BOOT_FAIL`: `client-ready`
/// fails right after the WebView navigated to the sidecar, exercising the
/// navigate-back-to-splash error path.
fn inject_boot_fault(point: &str) -> bool {
    std::env::var("DSH_DESKTOP_BOOT_FAIL").ok().as_deref() == Some(point)
}

fn show_migration(
    window: &tauri::WebviewWindow,
    splash: Option<&tauri::Url>,
    report: &MigrationReport,
) {
    let summary = format!(
        "Copied {}.
Skipped credentials.
Backup: {}",
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
    show_on_splash(window, splash, "window.__DSH_SHOW_MIGRATION__", &summary);
}

fn show_error(window: &tauri::WebviewWindow, splash: Option<&tauri::Url>, message: &str) {
    show_on_splash(window, splash, "window.__DSH_SHOW_ERROR__", message);
}

/// Resource URL of the bundled start page, polled until the WebView stops
/// serving `about:blank`. Captured rather than hardcoded because the asset
/// scheme differs per platform (`tauri://localhost` vs `http://tauri.localhost`).
fn splash_url(window: &tauri::WebviewWindow) -> Option<tauri::Url> {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(url) = window.url() {
            let text = url.as_str();
            if !text.is_empty() && !text.starts_with("about:") && text != "null" {
                return Some(url);
            }
        }
        if std::time::Instant::now() >= deadline {
            return None;
        }
        thread::sleep(Duration::from_millis(50));
    }
}

/// Surface `message` through a start-page hook.
///
/// The hooks exist only on the bundled start page, so navigate back to it when
/// the WebView sits on the (possibly dead) sidecar origin, and keep retrying
/// the eval for a bounded window: a fast failure can race the start page's own
/// script load, and repeated idempotent evals are harmless. The error page is
/// also the one surface that must never stay hidden: the window is shown and
/// focused before the message lands.
fn show_on_splash(
    window: &tauri::WebviewWindow,
    splash: Option<&tauri::Url>,
    hook: &str,
    message: &str,
) {
    if let Some(splash) = splash {
        let on_splash = window.url().is_ok_and(|current| current == *splash);
        if !on_splash && window.navigate(splash.clone()).is_ok() {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while std::time::Instant::now() < deadline {
                if window.url().is_ok_and(|current| current == *splash) {
                    break;
                }
                thread::sleep(Duration::from_millis(50));
            }
        }
    }
    let _ = window.show();
    let _ = window.set_focus();
    let encoded = serde_json::to_string(message).unwrap_or_else(|_| "\"(boot failed)\"".into());
    let script = format!("{hook} && {hook}({encoded})");
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        let _ = window.eval(&script);
        thread::sleep(Duration::from_millis(250));
    }
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

/// Navigate the main WebView to the sidecar, handing the page its one-time
/// bootstrap nonce in the URL fragment.
///
/// The fragment is the delivery channel because user agents never put it on the
/// wire: serving the nonce inside the index would instead expose it to every
/// local process able to reach the loopback port, which carries no user
/// identity. The page strips it from session history once read.
///
/// # Parameters
/// - `window`: main WebView to navigate.
/// - `port`: sidecar loopback port.
/// - `nonce`: one-time bootstrap nonce; hex from [`generate_desktop_token`], so
///   it needs no percent-encoding.
///
/// # Returns
/// `Ok` once the WebView reports the sidecar origin, otherwise the failure.
fn navigate_to_sidecar(
    window: &tauri::WebviewWindow,
    port: u16,
    nonce: &str,
) -> Result<(), String> {
    let url = format!("http://127.0.0.1:{port}/#dshd-nonce={nonce}");
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
