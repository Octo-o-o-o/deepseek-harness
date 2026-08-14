//! Stale sidecar pid file. SIGKILL cannot reap orphans; the next boot can.

use std::fs;
use std::path::Path;
use std::process::Command;

/// One parsed `sidecar.pid` record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidecarRecord {
    /// Recorded process id.
    pub pid: u32,
    /// Entry script the sidecar was launched with (`bin.js` path).
    pub bin: String,
    /// Windows creation-time token; `0` when absent.
    #[cfg(windows)]
    pub start: u64,
}

/// Reap a leftover sidecar recorded in `<home>/sidecar.pid`.
///
/// `$DSH_HOME` is shared with the npm CLI, so a process id alone cannot
/// authorize a kill: after pid reuse it would name an unrelated `dsh web` the
/// user started themselves. Windows matches the recorded creation time
/// (pid reuse changes it); Unix matches the recorded entry script against
/// `ps`. A record whose identity cannot be verified is discarded unreaped.
///
/// # Parameters
/// - `home`: desktop `DSH_HOME`.
pub fn reap_stale_sidecar(home: &Path) {
    let path = home.join("sidecar.pid");
    let Ok(raw) = fs::read_to_string(&path) else {
        return;
    };
    if let Some(record) = parse_sidecar_record(&raw) {
        if matches_live_identity(&record) {
            terminate_pid(record.pid);
        }
    }
    let _ = fs::remove_file(&path);
}

/// Whether the live process at `record.pid` is the recorded sidecar.
fn matches_live_identity(record: &SidecarRecord) -> bool {
    #[cfg(windows)]
    {
        crate::job::process_creation_time(record.pid) == Some(record.start)
    }
    #[cfg(not(windows))]
    {
        command_of(record.pid).is_some_and(|command| command.contains(&record.bin))
    }
}

/// Split a pid file into the recorded identity.
///
/// Windows records carry a third line (creation time); a two-line record on
/// Windows predates that and is never reaped. Unix records are two lines.
///
/// # Parameters
/// - `raw`: pid file contents.
///
/// # Returns
/// The record, or `None` when a required line is missing or unparseable.
pub fn parse_sidecar_record(raw: &str) -> Option<SidecarRecord> {
    let mut lines = raw.lines();
    let pid = lines.next()?.trim().parse::<u32>().ok()?;
    let bin = lines.next()?.trim();
    if bin.is_empty() {
        return None;
    }
    #[cfg(windows)]
    {
        let start = lines.next()?.trim().parse::<u64>().ok()?;
        Some(SidecarRecord {
            pid,
            bin: bin.to_string(),
            start,
        })
    }
    #[cfg(not(windows))]
    {
        Some(SidecarRecord {
            pid,
            bin: bin.to_string(),
        })
    }
}

/// Record the live sidecar pid, entry script, and (Windows) creation time.
///
/// # Parameters
/// - `home`: desktop `DSH_HOME`.
/// - `pid`: sidecar process id.
/// - `bin`: `dsh` entry script path passed to Node.
/// - `start`: creation-time token on Windows, `None` elsewhere.
pub fn write_sidecar_pid(home: &Path, pid: u32, bin: &Path, start: Option<u64>) {
    let mut body = format!("{pid}\n{}\n", bin.display());
    if let Some(start) = start {
        body.push_str(&start.to_string());
        body.push('\n');
    }
    let _ = fs::write(home.join("sidecar.pid"), body);
}

/// Forget the pid file after a clean shutdown.
///
/// # Parameters
/// - `home`: desktop `DSH_HOME`.
pub fn clear_sidecar_pid(home: &Path) {
    let _ = fs::remove_file(home.join("sidecar.pid"));
}

#[cfg(not(windows))]
fn command_of(pid: u32) -> Option<String> {
    let output = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "command="])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn terminate_pid(pid: u32) {
    #[cfg(unix)]
    {
        // The stale sidecar was spawned with process_group(0): pid == pgid.
        // Escalate exactly like a live shutdown: TERM the group, then KILL.
        let pgid = pid as i32;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        // SAFETY: pgid names a process group this shell created at boot.
        unsafe {
            let _ = libc::killpg(pgid, libc::SIGTERM);
            while crate::process::process_group_has_members(pgid)
                && std::time::Instant::now() < deadline
            {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            if crate::process::process_group_has_members(pgid) {
                let _ = libc::killpg(pgid, libc::SIGKILL);
            }
        }
    }
    #[cfg(windows)]
    {
        // Explicit null stdio: combining CREATE_NO_WINDOW with inherited
        // console handles can fail the spawn under concurrency, and the
        // reaping path must not depend on what the caller happens to own.
        let mut command = Command::new("taskkill");
        command
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        crate::process::hide_child_console(&mut command);
        if let Err(err) = command.status() {
            eprintln!("desktop: reaping taskkill for pid {pid} failed: {err}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Stdio;

    /// Unique temp dir per call: test threads share one process id, so
    /// pid-based names collide across parallel tests (paths.rs uses the same
    /// nanos-plus-counter scheme).
    fn temp_dir() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "dsh-pid-{nanos}-{}",
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn missing_pid_file_is_a_no_op() {
        let dir = temp_dir();
        reap_stale_sidecar(&dir);
        write_sidecar_pid(&dir, 1, Path::new("/opt/dshd/app/lib/bin.js"), None);
        assert_eq!(
            fs::read_to_string(dir.join("sidecar.pid")).unwrap(),
            "1\n/opt/dshd/app/lib/bin.js\n"
        );
        clear_sidecar_pid(&dir);
        assert!(!dir.join("sidecar.pid").exists());
        let _ = fs::remove_dir_all(dir);
    }

    /// A record naming only a pid cannot prove the live process is ours.
    #[cfg(not(windows))]
    #[test]
    fn a_record_without_an_entry_script_is_not_reapable() {
        assert_eq!(parse_sidecar_record("4242\n"), None);
        assert_eq!(parse_sidecar_record("4242\n   \n"), None);
        assert_eq!(parse_sidecar_record("not-a-pid\n/x/bin.js\n"), None);
        assert_eq!(
            parse_sidecar_record("4242\n/x/bin.js\n"),
            Some(SidecarRecord {
                pid: 4242,
                bin: "/x/bin.js".into(),
            })
        );
    }

    /// Windows records need the creation-time line; two-line records predate
    /// it and stay unreapable.
    #[cfg(windows)]
    #[test]
    fn windows_records_require_a_creation_time() {
        assert_eq!(parse_sidecar_record("4242\n/x/bin.js\n"), None);
        assert_eq!(parse_sidecar_record("4242\n/x/bin.js\nnot-a-time\n"), None);
        assert_eq!(
            parse_sidecar_record("4242\n/x/bin.js\n1337\n"),
            Some(SidecarRecord {
                pid: 4242,
                bin: "/x/bin.js".into(),
                start: 1337,
            })
        );
    }

    /// The desktop must never kill a `dsh web` the user started from npx.
    #[cfg(unix)]
    #[test]
    fn a_foreign_dsh_web_at_the_recorded_pid_survives() {
        let dir = temp_dir();
        let mut foreign = Command::new("/bin/sh")
            .args(["-c", "exec sleep 30 # node /npx/dsh/lib/bin.js web"])
            .spawn()
            .unwrap();
        write_sidecar_pid(
            &dir,
            foreign.id(),
            Path::new("/opt/dshd/app/lib/bin.js"),
            None,
        );
        reap_stale_sidecar(&dir);
        assert!(
            foreign.try_wait().unwrap().is_none(),
            "reap killed a process whose entry script does not match the record"
        );
        foreign.kill().unwrap();
        let _ = foreign.wait();
        let _ = fs::remove_dir_all(dir);
    }

    /// Windows identity: a matching creation time is killed, a mismatched one
    /// survives, and `taskkill` clears the matching process.
    #[cfg(windows)]
    #[test]
    fn windows_identity_governs_reaping() {
        let dir = temp_dir();
        let mut victim = Command::new("ping")
            .args(["-n", "30", "127.0.0.1"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let token = crate::job::process_creation_time(victim.id()).unwrap();
        write_sidecar_pid(&dir, victim.id(), Path::new("C:/dshd/bin.js"), Some(token));
        reap_stale_sidecar(&dir);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while victim.try_wait().unwrap().is_none() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(
            victim.try_wait().unwrap().is_some(),
            "reap did not kill the pid with a matching creation time"
        );

        let mut survivor = Command::new("ping")
            .args(["-n", "30", "127.0.0.1"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        write_sidecar_pid(
            &dir,
            survivor.id(),
            Path::new("C:/dshd/bin.js"),
            Some(token.wrapping_add(12345)),
        );
        reap_stale_sidecar(&dir);
        assert!(
            survivor.try_wait().unwrap().is_none(),
            "reap killed a pid whose creation time does not match the record"
        );
        let _ = Command::new("taskkill")
            .args(["/PID", &survivor.id().to_string(), "/T", "/F"])
            .status();
        let _ = survivor.wait();
        let _ = fs::remove_dir_all(dir);
    }
}
