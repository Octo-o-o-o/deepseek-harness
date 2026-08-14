//! Stale sidecar pid file. SIGKILL cannot reap orphans; the next boot can.

use std::fs;
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::Duration;

/// Reap a leftover sidecar recorded in `<home>/sidecar.pid`.
///
/// `$DSH_HOME` is shared with the npm CLI, so a process id alone cannot
/// authorize a kill: after pid reuse it would name an unrelated `dsh web` the
/// user started themselves. The recorded entry script must also match, and a
/// record without one is discarded unreaped.
///
/// # Parameters
/// - `home`: desktop `DSH_HOME`.
pub fn reap_stale_sidecar(home: &Path) {
    let path = home.join("sidecar.pid");
    let Ok(raw) = fs::read_to_string(&path) else {
        return;
    };
    if let Some((pid, bin)) = parse_sidecar_record(&raw) {
        if command_of(pid).is_some_and(|command| command.contains(bin)) {
            terminate_pid(pid);
        }
    }
    let _ = fs::remove_file(&path);
}

/// Split a pid file into the recorded process id and entry script.
///
/// # Parameters
/// - `raw`: pid file contents.
///
/// # Returns
/// The pair, or `None` when either line is missing or unparseable.
pub fn parse_sidecar_record(raw: &str) -> Option<(u32, &str)> {
    let mut lines = raw.lines();
    let pid = lines.next()?.trim().parse::<u32>().ok()?;
    let bin = lines.next()?.trim();
    if bin.is_empty() {
        return None;
    }
    Some((pid, bin))
}

/// Record the live sidecar pid and the entry script it was launched with.
///
/// # Parameters
/// - `home`: desktop `DSH_HOME`.
/// - `pid`: sidecar process id.
/// - `bin`: `dsh` entry script path passed to Node.
pub fn write_sidecar_pid(home: &Path, pid: u32, bin: &Path) {
    let _ = fs::write(
        home.join("sidecar.pid"),
        format!("{pid}\n{}\n", bin.display()),
    );
}

/// Forget the pid file after a clean shutdown.
///
/// # Parameters
/// - `home`: desktop `DSH_HOME`.
pub fn clear_sidecar_pid(home: &Path) {
    let _ = fs::remove_file(home.join("sidecar.pid"));
}

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
        unsafe {
            libc::kill(pid as i32, libc::SIGTERM);
        }
        thread::sleep(Duration::from_millis(200));
        unsafe {
            libc::kill(pid as i32, libc::SIGKILL);
        }
    }
    #[cfg(not(unix))]
    {
        let _ = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .status();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_pid_file_is_a_no_op() {
        let dir = std::env::temp_dir().join(format!("dsh-pid-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        reap_stale_sidecar(&dir);
        write_sidecar_pid(&dir, 1, Path::new("/opt/dshd/app/lib/bin.js"));
        assert_eq!(
            fs::read_to_string(dir.join("sidecar.pid")).unwrap(),
            "1\n/opt/dshd/app/lib/bin.js\n"
        );
        clear_sidecar_pid(&dir);
        assert!(!dir.join("sidecar.pid").exists());
        let _ = fs::remove_dir_all(dir);
    }

    /// A record naming only a pid cannot prove the live process is ours.
    #[test]
    fn a_record_without_an_entry_script_is_not_reapable() {
        assert_eq!(parse_sidecar_record("4242\n"), None);
        assert_eq!(parse_sidecar_record("4242\n   \n"), None);
        assert_eq!(parse_sidecar_record("not-a-pid\n/x/bin.js\n"), None);
        assert_eq!(
            parse_sidecar_record("4242\n/x/bin.js\n"),
            Some((4242, "/x/bin.js"))
        );
    }

    /// The desktop must never kill a `dsh web` the user started from npx.
    #[test]
    fn a_foreign_dsh_web_at_the_recorded_pid_survives() {
        let dir = std::env::temp_dir().join(format!("dsh-pid-foreign-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let mut foreign = Command::new("/bin/sh")
            .args(["-c", "exec sleep 30 # node /npx/dsh/lib/bin.js web"])
            .spawn()
            .unwrap();
        write_sidecar_pid(&dir, foreign.id(), Path::new("/opt/dshd/app/lib/bin.js"));
        reap_stale_sidecar(&dir);
        assert!(
            foreign.try_wait().unwrap().is_none(),
            "reap killed a process whose entry script does not match the record"
        );
        foreign.kill().unwrap();
        let _ = foreign.wait();
        let _ = fs::remove_dir_all(dir);
    }
}
