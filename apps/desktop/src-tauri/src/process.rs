//! Child-console suppression and process-tree termination. Unix escalation
//! probes process-group liveness; Windows relies on the Job Object assigned at
//! spawn (direct-child fallback when the job could not be created).

use std::process::Command;
use std::time::{Duration, Instant};

/// Process-creation flag `CREATE_NO_WINDOW` from `CreateProcessW`.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Suppress the console window a Windows child would otherwise allocate.
///
/// The release shell runs under the GUI subsystem, so a console-subsystem
/// child (`node.exe`, `taskkill.exe`) spawned without this flag receives a
/// fresh visible console. The sidecar's own children inherit its windowless
/// console (`dsh-subprocess-local` never sets `detached` on Windows), so one
/// flag at the sidecar spawn covers the whole tree. No-op on other platforms.
///
/// # Parameters
/// - `command`: builder for the child about to spawn.
pub fn hide_child_console(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    {
        let _ = command;
    }
}

/// Operations the shutdown sequence needs from a live child tree.
pub trait ProcessTree {
    /// Ask the tree to drain (SIGTERM / best-effort Windows terminate).
    fn signal_terminate(&mut self);
    /// Forcibly destroy the tree (SIGKILL / Job Object kill).
    fn signal_kill(&mut self);
    /// Whether any process in the tree is still running.
    fn is_alive(&mut self) -> bool;
    /// Reap a dead child so it does not stay a zombie.
    fn reap(&mut self);
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
        let reap_until = now() + Duration::from_secs(2);
        while tree.is_alive() && now() < reap_until {
            sleep_for(Duration::from_millis(20));
        }
    }
    tree.reap();
}

/// Whether any process remains in `pgid`.
///
/// `kill(0)` reports permission to signal without sending one, which for a
/// group the shell created is exactly "the group still exists".
///
/// # Parameters
/// - `pgid`: process group id, which for this shell is the sidecar's pid.
///
/// # Returns
/// `true` while the group has at least one member.
#[cfg(unix)]
pub fn process_group_has_members(pgid: i32) -> bool {
    // SAFETY: signal 0 performs error checking only and never delivers.
    let rc = unsafe { libc::killpg(pgid, 0) };
    if rc == 0 {
        return true;
    }
    // EPERM means members exist that this process may not signal; only ESRCH
    // says the group is gone.
    std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
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
    /// The spawned sidecar process, which leads its own process group.
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
        // The leader is asked first because reaping it is what stops the kernel
        // from keeping a zombie, but the answer is about the group: the sidecar
        // starts bash, pwsh, and picker processes in it, and one of those
        // outliving a leader that exited on SIGTERM is exactly the case the
        // forced kill exists for.
        if matches!(self.child.try_wait(), Ok(None)) {
            return true;
        }
        #[cfg(unix)]
        {
            process_group_has_members(self.child.id() as i32)
        }
        #[cfg(not(unix))]
        {
            false
        }
    }

    fn reap(&mut self) {
        let _ = self.child.try_wait();
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

        fn reap(&mut self) {}
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

    /// `CREATE_NO_WINDOW` must stay a valid creation flag: an invalid value
    /// fails every Windows child spawn.
    #[cfg(windows)]
    #[test]
    fn hidden_console_children_still_spawn() {
        let mut command = Command::new("cmd");
        hide_child_console(&mut command);
        command.args(["/C", "exit 42"]);
        assert_eq!(command.status().unwrap().code(), Some(42));
    }

    /// taskkill runs unchanged under `CREATE_NO_WINDOW`: the reaping path must
    /// still terminate the recorded pid.
    #[cfg(windows)]
    #[test]
    fn hidden_console_taskkill_still_kills() {
        let mut victim = Command::new("ping")
            .args(["-n", "30", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let mut command = Command::new("taskkill");
        command.args(["/PID", &victim.id().to_string(), "/T", "/F"]);
        hide_child_console(&mut command);
        let status = command.status().unwrap();
        assert!(status.success(), "taskkill failed: {status}");
        let deadline = Instant::now() + Duration::from_secs(10);
        while victim.try_wait().unwrap().is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(
            victim.try_wait().unwrap().is_some(),
            "hidden taskkill did not kill the pid"
        );
    }

    /// A TERM-trapping grandchild survives the leader: escalation must fire
    /// on group liveness, and the group must be gone afterwards. Mirrors the
    /// TERM-trapping case in `dsh-subprocess-local`.
    #[cfg(unix)]
    #[test]
    fn escalates_and_kills_a_term_trapping_group() {
        use std::os::unix::process::CommandExt;
        let mut child = Command::new("sh")
            .arg("-c")
            .arg("trap '' TERM; sleep 30 & wait")
            .process_group(0)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let pgid = child.id() as i32;
        let mut tree = ChildTree { child: &mut child };
        let started = Instant::now();
        shutdown_tree(
            &mut tree,
            Duration::from_millis(300),
            Instant::now,
            std::thread::sleep,
        );
        let elapsed = started.elapsed();
        assert!(
            elapsed < Duration::from_secs(3),
            "shutdown took {elapsed:?}, expected a fast group escalation"
        );
        assert!(
            !process_group_has_members(pgid),
            "TERM-trapping group survived the forced kill"
        );
    }
}
