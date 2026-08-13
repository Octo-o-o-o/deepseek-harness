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
mod overlay;
mod paths;
mod process;
mod ready;
mod sidecar;
mod state;
mod supervisor;
mod token;
mod tray;

use std::path::Path;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use tauri::Manager;

use crate::health::{check_host_described, check_loader_ready, check_websockets_ready};
use crate::lock::{try_lock_home, HomeLock};
use crate::logs::{install_panic_hook, open_logs_dir, rotate_sidecar_log};
use crate::migrate::{
    default_legacy_home, inject_fault_from_env, migrate_legacy_home, MigrationReport,
};
use crate::paths::{
    default_dsh_home, default_workspace_cwd, ensure_desktop_state, resolve_node, resolve_web_bin,
};
use crate::sidecar::{desktop_web_args, spawn_sidecar, wait_ready, SidecarSpec};
use crate::state::{transition, BootEvent, BootPhase};
use crate::supervisor::SidecarSupervisor;
use crate::token::generate_desktop_token;
use crate::tray::install_tray;

/// Shared runtime state held by the Tauri app.
pub struct AppState {
    supervisor: SidecarSupervisor,
    boot: Mutex<Option<thread::JoinHandle<()>>>,
    _lock: Mutex<Option<HomeLock>>,
}

impl AppState {
    /// Unique stop entry: cancel boot, take the sidecar out of the lock, shut it down.
    pub fn request_stop(&self) {
        self.supervisor.request_stop();
    }

    fn join_boot(&self) {
        if let Ok(mut guard) = self.boot.lock() {
            if let Some(handle) = guard.take() {
                let _ = handle.join();
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
            let window = app
                .get_webview_window("main")
                .ok_or("desktop: main window missing")?;
            let handle = app.handle().clone();
            let boot = thread::spawn(move || {
                let state = handle.state::<AppState>();
                let exe = std::env::current_exe().unwrap_or_else(|_| Path::new(".").to_path_buf());
                let cwd = std::env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf());
                if let Err(message) = boot_and_navigate(&window, &state, &exe, &cwd) {
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
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                if let Some(state) = app.try_state::<AppState>() {
                    state.request_stop();
                    state.join_boot();
                }
            }
        });
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
    let report = migrate_legacy_home(&default_legacy_home(), &home, inject_fault_from_env())
        .map_err(|err| err.to_string())?;
    if report.migrated {
        show_migration(window, &report);
    }
    let workspace = default_workspace_cwd(&home);
    ensure_desktop_state(&home, &workspace).map_err(|err| err.to_string())?;
    let token = generate_desktop_token().map_err(|err| err.to_string())?;
    let mut env = vec![("DSH_HOME".into(), home.to_string_lossy().into_owned())];
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
        args: desktop_web_args(&bin, &["--desktop-token".into(), token.clone()]),
        cwd: workspace,
        env,
        log_path: home.join("logs/sidecar.log"),
    };
    let mut phase = BootPhase::Idle;
    let spawned = spawn_sidecar(&spec).map_err(|err| err.to_string())?;
    phase = transition(phase, BootEvent::SpawnOk);
    let (process, ready) = spawned.into_parts();
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
    if let Err(err) = check_websockets_ready(port, &token) {
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
    if let Err(err) = navigate_to_sidecar(window, port) {
        state.request_stop();
        return Err(err);
    }
    let _ = transition(phase, BootEvent::Visible);
    Ok(())
}

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
    let encoded = serde_json::to_string(message).unwrap_or_else(|_| "\"boot failed\"".into());
    let script = format!("window.__DSH_SHOW_ERROR__ && window.__DSH_SHOW_ERROR__({encoded})");
    let _ = window.eval(&script);
}

fn navigate_to_sidecar(window: &tauri::WebviewWindow, port: u16) -> Result<(), String> {
    let url = format!("http://127.0.0.1:{port}/");
    let parsed = tauri::Url::parse(&url).map_err(|err| err.to_string())?;
    window.navigate(parsed).map_err(|err| err.to_string())?;
    let expected = url.trim_end_matches('/');
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    while std::time::Instant::now() < deadline {
        if let Ok(current) = window.url() {
            if current.as_str().starts_with(expected) {
                return Ok(());
            }
        }
        thread::sleep(Duration::from_millis(50));
    }
    Err("webview navigation did not finish".into())
}
