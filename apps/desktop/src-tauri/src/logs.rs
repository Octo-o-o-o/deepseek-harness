//! Sidecar log rotation, panic hook, and opening the log directory.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Rotate `sidecar.log` once it reaches this size.
pub const SIDECAR_LOG_MAX_BYTES: u64 = 50 * 1024 * 1024;

/// Rename `sidecar.log` to `sidecar.log.1` when it is at least 50MB.
///
/// # Parameters
/// - `path`: intended sidecar log path.
///
/// # Returns
/// `Ok(())` after rotation or when no rotation is needed.
pub fn rotate_sidecar_log(path: &Path) -> io::Result<()> {
    if let Ok(meta) = fs::metadata(path) {
        if meta.len() >= SIDECAR_LOG_MAX_BYTES {
            let rotated = path.with_file_name("sidecar.log.1");
            let _ = fs::remove_file(&rotated);
            fs::rename(path, rotated)?;
        }
    }
    Ok(())
}

/// Install a panic hook that appends to `<home>/logs/crash.log`.
///
/// # Parameters
/// - `home`: desktop `DSH_HOME`.
pub fn install_panic_hook(home: PathBuf) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let path = home.join("logs/crash.log");
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let body = format!("{info}\n");
        let _ = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .and_then(|mut file| {
                use std::io::Write;
                file.write_all(body.as_bytes())
            });
        previous(info);
    }));
}

/// Open the desktop log directory in the host file manager.
///
/// # Parameters
/// - `home`: desktop `DSH_HOME`.
///
/// # Returns
/// `Ok(())` when the file manager process started.
pub fn open_logs_dir(home: &Path) -> io::Result<()> {
    let logs = home.join("logs");
    fs::create_dir_all(&logs)?;
    open_dir(&logs)
}

fn open_dir(path: &Path) -> io::Result<()> {
    let mut command = open_command();
    command.arg(path).spawn()?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn open_command() -> Command {
    Command::new("open")
}

#[cfg(target_os = "windows")]
fn open_command() -> Command {
    Command::new("explorer")
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn open_command() -> Command {
    Command::new("xdg-open")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotates_when_over_max() {
        let dir = std::env::temp_dir().join(format!("dsh-logs-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sidecar.log");
        fs::write(&path, vec![b'x'; 64]).unwrap();
        rotate_sidecar_log(&path).unwrap();
        assert!(path.is_file());
        fs::write(&path, vec![b'y'; (SIDECAR_LOG_MAX_BYTES as usize) + 8]).unwrap();
        rotate_sidecar_log(&path).unwrap();
        assert!(!path.exists());
        assert!(dir.join("sidecar.log.1").is_file());
        let _ = fs::remove_dir_all(dir);
    }
}
