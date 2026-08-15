//! Application update check and install.
//!
//! The shell owns this end to end: the update entry lives in the tray menu, not
//! in the Web UI, because that UI is shared with the browser surface where an
//! in-app updater has no meaning.
//!
//! Trust comes from the minisign key pair configured in `tauri.conf.json`. The
//! plugin verifies the detached signature of each downloaded artifact against
//! the bundled public key before writing anything, so a compromised endpoint
//! can withhold or replay updates but cannot ship code of its own.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::{AppHandle, Manager};
use tauri_plugin_updater::UpdaterExt;

use crate::AppState;

/// Delay before the first automatic check, so it does not compete with sidecar
/// startup for CPU or network on a cold launch.
const FIRST_CHECK_DELAY: Duration = Duration::from_secs(30);

/// Interval between automatic checks for a long-running session.
const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

/// What the last completed check found, for the tray to render.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateStatus {
    /// No check has completed yet this launch.
    Unknown,
    /// The running build is current.
    UpToDate,
    /// A newer version is published.
    Available {
        /// Version string reported by the endpoint.
        version: String,
    },
    /// A download or install is running.
    Installing,
}

/// Live update state shared by the checker and the tray.
pub struct UpdateState {
    status: Mutex<UpdateStatus>,
    /// Held for the whole download+install, so a second click is ignored rather
    /// than starting a concurrent install of the same artifact.
    busy: AtomicBool,
}

impl UpdateState {
    /// Create the state with no check performed yet.
    pub fn new() -> Self {
        Self {
            status: Mutex::new(UpdateStatus::Unknown),
            busy: AtomicBool::new(false),
        }
    }

    /// Read the latest known status.
    ///
    /// # Returns
    /// The status recorded by the most recent completed check.
    pub fn status(&self) -> UpdateStatus {
        self.status.lock().expect("update status mutex").clone()
    }

    fn set(&self, status: UpdateStatus) {
        *self.status.lock().expect("update status mutex") = status;
    }
}

impl Default for UpdateState {
    fn default() -> Self {
        Self::new()
    }
}

/// Start the background check loop.
///
/// Failures stay silent by design: an offline machine or an endpoint outage
/// must not interrupt the session, so the tray keeps reporting the last known
/// status and the next tick tries again.
///
/// # Parameters
/// - `app`: handle used to reach the updater and the tray.
pub fn spawn_checker(app: AppHandle) {
    // A plain thread rather than an async timer: the loop sleeps for hours, and
    // this keeps the crate off a separate async timer dependency.
    std::thread::spawn(move || {
        std::thread::sleep(FIRST_CHECK_DELAY);
        loop {
            tauri::async_runtime::block_on(check(&app));
            std::thread::sleep(CHECK_INTERVAL);
        }
    });
}

/// Run one check and record the outcome.
///
/// # Parameters
/// - `app`: handle used to reach the updater.
pub async fn check(app: &AppHandle) {
    let state = app.state::<Arc<UpdateState>>();
    // A check during an install would overwrite the Installing status the tray
    // is showing, and its result would be stale the moment the restart lands.
    if state.busy.load(Ordering::SeqCst) {
        return;
    }
    let updater = match app.updater() {
        Ok(updater) => updater,
        Err(error) => {
            eprintln!("desktop: updater unavailable: {error}");
            return;
        }
    };
    match updater.check().await {
        Ok(Some(update)) => {
            let version = update.version.clone();
            println!("desktop: update available: {version}");
            state.set(UpdateStatus::Available { version });
        }
        Ok(None) => state.set(UpdateStatus::UpToDate),
        Err(error) => eprintln!("desktop: update check failed: {error}"),
    }
    crate::tray::refresh_update_item(app);
}

/// Download, install, and restart into the new version.
///
/// The sidecar is stopped first: the installer replaces the bundled Node
/// runtime and the deployed CLI underneath a running process, and on Windows an
/// open file cannot be replaced at all.
///
/// # Parameters
/// - `app`: handle used to reach the updater and the sidecar state.
pub async fn install_and_restart(app: AppHandle) {
    let state = app.state::<Arc<UpdateState>>();
    if state.busy.swap(true, Ordering::SeqCst) {
        return;
    }
    state.set(UpdateStatus::Installing);
    crate::tray::refresh_update_item(&app);
    let outcome = run_install(&app).await;
    if let Err(message) = outcome {
        eprintln!("desktop: {message}");
        state.set(UpdateStatus::Unknown);
        state.busy.store(false, Ordering::SeqCst);
        crate::tray::refresh_update_item(&app);
    }
    // The success path never returns: `restart()` replaces the process.
}

async fn run_install(app: &AppHandle) -> Result<(), String> {
    let updater = app
        .updater()
        .map_err(|error| format!("updater unavailable: {error}"))?;
    let update = updater
        .check()
        .await
        .map_err(|error| format!("update check failed: {error}"))?
        .ok_or_else(|| "no update available".to_string())?;
    // Quiesce before the installer touches the payload.
    app.state::<AppState>().request_stop();
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|error| format!("update install failed: {error}"))?;
    println!("desktop: update installed; restarting");
    app.restart();
}
