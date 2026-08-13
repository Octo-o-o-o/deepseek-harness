//! Process-tree termination. Unix uses a process group; Windows is a stub.

use std::time::{Duration, Instant};

/// Operations the shutdown sequence needs from a live child tree.
pub trait ProcessTree {
    /// Ask the tree to drain (SIGTERM / best-effort Windows terminate).
    fn signal_terminate(&mut self);
    /// Forcibly destroy the tree (SIGKILL / Job Object kill).
    fn signal_kill(&mut self);
    /// Whether any process in the tree is still running.
    fn is_alive(&mut self) -> bool;
}

/// Send SIGTERM, wait up to `grace`, then SIGKILL if the tree is still alive.
///
/// # Parameters
/// - `tree`: injectable process-tree handle.
/// - `grace`: maximum wait after terminate before a forced kill.
/// - `now`: clock used to enforce `grace`.
/// - `sleep_for`: wait primitive used between polls.
pub fn shutdown_tree<T, N, S>(tree: &mut T, grace: Duration, now: N, sleep_for: S)
where
    T: ProcessTree + ?Sized,
    N: Fn() -> Instant,
    S: Fn(Duration),
{
    tree.signal_terminate();
    let deadline = now() + grace;
    while tree.is_alive() && now() < deadline {
        sleep_for(Duration::from_millis(20));
    }
    if tree.is_alive() {
        tree.signal_kill();
    }
}

/// Send `sig` to every process in `pgid`.
///
/// # Safety
/// `pgid` must identify a process group this process is allowed to signal.
#[cfg(unix)]
pub unsafe fn kill_process_group(pgid: i32, sig: i32) -> std::io::Result<()> {
    let rc = unsafe { libc::killpg(pgid, sig) };
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// Live `std::process::Child` plus its Unix process group or Windows job.
pub struct ChildTree<'a> {
    /// The spawned sidecar process.
    pub child: &'a mut std::process::Child,
    /// Windows Job Object assigned at spawn. Not locally verified.
    #[cfg(windows)]
    pub job: Option<&'a crate::job::JobObject>,
}

impl ProcessTree for ChildTree<'_> {
    fn signal_terminate(&mut self) {
        #[cfg(unix)]
        {
            let _ = unsafe { kill_process_group(self.child.id() as i32, libc::SIGTERM) };
        }
        #[cfg(windows)]
        {
            // Not locally verified on this machine. Prefer the Job Object
            // assigned at spawn so bash/pwsh/picker grandchildren die too.
            if let Some(job) = self.job.as_ref() {
                job.terminate();
            } else {
                let _ = self.child.kill();
            }
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = self.child.kill();
        }
    }

    fn signal_kill(&mut self) {
        #[cfg(unix)]
        {
            let _ = unsafe { kill_process_group(self.child.id() as i32, libc::SIGKILL) };
        }
        #[cfg(windows)]
        {
            if let Some(job) = self.job.as_ref() {
                job.terminate();
            } else {
                let _ = self.child.kill();
            }
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = self.child.kill();
        }
    }

    fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    struct FakeTree {
        alive: Cell<bool>,
        terminate: Cell<u32>,
        kill: Cell<u32>,
        die_on_terminate: bool,
    }

    impl ProcessTree for FakeTree {
        fn signal_terminate(&mut self) {
            self.terminate.set(self.terminate.get() + 1);
            if self.die_on_terminate {
                self.alive.set(false);
            }
        }

        fn signal_kill(&mut self) {
            self.kill.set(self.kill.get() + 1);
            self.alive.set(false);
        }

        fn is_alive(&mut self) -> bool {
            self.alive.get()
        }
    }

    #[test]
    fn graceful_exit_skips_forced_kill() {
        let mut tree = FakeTree {
            alive: Cell::new(true),
            terminate: Cell::new(0),
            kill: Cell::new(0),
            die_on_terminate: true,
        };
        shutdown_tree(&mut tree, Duration::from_secs(5), Instant::now, |_| {});
        assert_eq!(tree.terminate.get(), 1);
        assert_eq!(tree.kill.get(), 0);
    }

    #[test]
    fn forced_kill_runs_after_grace() {
        let mut tree = FakeTree {
            alive: Cell::new(true),
            terminate: Cell::new(0),
            kill: Cell::new(0),
            die_on_terminate: false,
        };
        shutdown_tree(&mut tree, Duration::ZERO, Instant::now, |_| {});
        assert_eq!(tree.terminate.get(), 1);
        assert_eq!(tree.kill.get(), 1);
        assert!(!tree.alive.get());
    }
}
