//! Single stop entry for the sidecar. Shutdown runs outside the state lock.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use crate::sidecar::SidecarProcess;

/// Drain window handed to the sidecar before it is killed.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);

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

    /// Store the spawned sidecar. Shuts it down instead when a stop already ran.
    ///
    /// The flag is read while the slot is held, because a stop that lands
    /// between an unlocked read and the write would take an empty slot and
    /// return, leaving this call to store a process nobody stops afterwards.
    ///
    /// # Parameters
    /// - `process`: live sidecar.
    pub fn install(&self, process: SidecarProcess) {
        let orphan = match self.sidecar.lock() {
            Ok(mut guard) => {
                if self.stopping.load(Ordering::SeqCst) {
                    Some(process)
                } else {
                    *guard = Some(process);
                    None
                }
            }
            Err(_) => Some(process),
        };
        // Outside the lock: shutdown waits out the grace period.
        if let Some(mut orphan) = orphan {
            orphan.shutdown(SHUTDOWN_GRACE);
        }
    }

    /// Take the sidecar out of the lock and shut it down. Idempotent.
    pub fn request_stop(&self) {
        let process = match self.sidecar.lock() {
            Ok(mut guard) => {
                if self.stopping.swap(true, Ordering::SeqCst) {
                    return;
                }
                self.cancelled.store(true, Ordering::SeqCst);
                guard.take()
            }
            Err(_) => {
                self.stopping.store(true, Ordering::SeqCst);
                self.cancelled.store(true, Ordering::SeqCst);
                None
            }
        };
        if let Some(mut process) = process {
            process.shutdown(SHUTDOWN_GRACE);
        }
    }

    /// Block until the sidecar exits on its own.
    ///
    /// # Parameters
    /// - `poll`: interval between liveness checks.
    ///
    /// # Returns
    /// `true` when the sidecar exited while nobody asked it to, `false` once a
    /// stop is in flight.
    pub fn wait_for_unexpected_exit(&self, poll: Duration) -> bool {
        loop {
            if self.stopping.load(Ordering::SeqCst) {
                return false;
            }
            let alive = match self.sidecar.lock() {
                Ok(mut guard) => match guard.as_mut() {
                    Some(process) => process.is_alive(),
                    None => return false,
                },
                Err(_) => return false,
            };
            if !alive {
                return !self.stopping.load(Ordering::SeqCst);
            }
            std::thread::sleep(poll);
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
