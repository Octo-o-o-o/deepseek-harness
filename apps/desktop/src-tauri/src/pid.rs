//! Stale sidecar pid file. SIGKILL cannot reap orphans; the next boot can.

use std::fs;
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::Duration;

/// Reap a leftover sidecar recorded in `<home>/sidecar.pid` if it still looks
/// like `dsh web`.
///
/// # Parameters
/// - `home`: desktop `DSH_HOME`.
pub fn reap_stale_sidecar(home: &Path) {
    let path = home.join("sidecar.pid");
    let Ok(raw) = fs::read_to_string(&path) else {
        return;
    };
    let Ok(pid) = raw.trim().parse::<u32>() else {
        let _ = fs::remove_file(&path);
        return;
    };
    if looks_like_sidecar(pid) {
        terminate_pid(pid);
    }
    let _ = fs::remove_file(&path);
}

/// Record the live sidecar pid.
///
/// # Parameters
/// - `home`: desktop `DSH_HOME`.
/// - `pid`: sidecar process id.
pub fn write_sidecar_pid(home: &Path, pid: u32) {
    let _ = fs::write(home.join("sidecar.pid"), format!("{pid}\n"));
}

/// Forget the pid file after a clean shutdown.
///
/// # Parameters
/// - `home`: desktop `DSH_HOME`.
pub fn clear_sidecar_pid(home: &Path) {
    let _ = fs::remove_file(home.join("sidecar.pid"));
}

fn looks_like_sidecar(pid: u32) -> bool {
    let output = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "command="])
        .output();
    let Ok(output) = output else {
        return false;
    };
    let command = String::from_utf8_lossy(&output.stdout);
    command.contains("web") && (command.contains("bin.js") || command.contains("dsh"))
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
        write_sidecar_pid(&dir, 1);
        assert!(dir.join("sidecar.pid").is_file());
        clear_sidecar_pid(&dir);
        assert!(!dir.join("sidecar.pid").exists());
        let _ = fs::remove_dir_all(dir);
    }
}
