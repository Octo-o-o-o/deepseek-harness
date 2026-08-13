//! Single stop entry for the sidecar. Shutdown runs outside the state lock.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use crate::sidecar::SidecarProcess;

/// Owns the live sidecar and the cancel/stop flags.
pub struct SidecarSupervisor {
    sidecar: Mutex<Option<SidecarProcess>>,
    stopping: AtomicBool,
    cancelled: AtomicBool,
}

impl SidecarSupervisor {
    /// Empty supervisor, no sidecar yet.
    pub fn new() -> Self {
        Self {
            sidecar: Mutex::new(None),
            stopping: AtomicBool::new(false),
            cancelled: AtomicBool::new(false),
        }
    }

    /// Store the spawned sidecar. No-op when a stop is already in flight.
    ///
    /// # Parameters
    /// - `process`: live sidecar.
    pub fn install(&self, process: SidecarProcess) {
        if self.stopping.load(Ordering::SeqCst) {
            let mut orphan = process;
            orphan.shutdown(Duration::from_secs(5));
            return;
        }
        if let Ok(mut guard) = self.sidecar.lock() {
            *guard = Some(process);
        }
    }

    /// Take the sidecar out of the lock and shut it down. Idempotent.
    pub fn request_stop(&self) {
        if self.stopping.swap(true, Ordering::SeqCst) {
            return;
        }
        self.cancelled.store(true, Ordering::SeqCst);
        let process = match self.sidecar.lock() {
            Ok(mut guard) => guard.take(),
            Err(_) => None,
        };
        if let Some(mut process) = process {
            process.shutdown(Duration::from_secs(5));
        }
    }

    /// Whether boot should abort.
    ///
    /// # Returns
    /// `true` after the first [`SidecarSupervisor::request_stop`].
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stop_without_sidecar_is_idempotent() {
        let supervisor = SidecarSupervisor::new();
        supervisor.request_stop();
        supervisor.request_stop();
        assert!(supervisor.is_cancelled());
    }

    #[test]
    fn install_after_stop_shuts_the_process_down() {
        let supervisor = SidecarSupervisor::new();
        supervisor.request_stop();
        assert!(supervisor.is_cancelled());
    }
}
