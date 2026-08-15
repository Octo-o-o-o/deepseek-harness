//! Sidecar log rotation, panic hook, and opening the log directory.

use std::fs;
use std::io;
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};

use crate::opener::open_path;

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

/// Lines of `sidecar.log` quoted into a boot-failure message.
const SIDECAR_LOG_TAIL_LINES: usize = 20;

/// Bytes read from the end of `sidecar.log` to find those lines. A fatal
/// Loader diagnostic is one long line plus a short stack, so this is generous
/// for the tail while bounding the read on a log grown to its rotation size.
const SIDECAR_LOG_TAIL_BYTES: u64 = 64 * 1024;

/// The last lines of `sidecar.log`, for a boot failure whose own error text
/// reports only that the sidecar stopped talking.
///
/// # Parameters
/// - `path`: sidecar log path.
///
/// # Returns
/// The trailing lines, or `None` when the log is unreadable or empty. A read
/// starting mid-UTF-8 loses at most the first partial line.
pub fn sidecar_log_tail(path: &Path) -> Option<String> {
    let mut file = fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let from = len.saturating_sub(SIDECAR_LOG_TAIL_BYTES);
    file.seek(io::SeekFrom::Start(from)).ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let mut lines: Vec<&str> = text.lines().collect();
    if from > 0 && !lines.is_empty() {
        lines.remove(0);
    }
    let tail = lines
        .iter()
        .rev()
        .take(SIDECAR_LOG_TAIL_LINES)
        .rev()
        .copied()
        .collect::<Vec<&str>>()
        .join("\n");
    if tail.is_empty() {
        return None;
    }
    Some(tail)
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
    open_path(&logs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tails_the_last_lines_and_reports_nothing_for_an_absent_log() {
        let dir = std::env::temp_dir().join(format!("dsh-logs-tail-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sidecar.log");
        assert_eq!(sidecar_log_tail(&path), None);
        fs::write(&path, "").unwrap();
        assert_eq!(sidecar_log_tail(&path), None);

        let body: String = (0..(SIDECAR_LOG_TAIL_LINES + 5))
            .map(|n| format!("line {n}\n"))
            .collect();
        fs::write(&path, &body).unwrap();
        let tail = sidecar_log_tail(&path).unwrap();
        assert_eq!(tail.lines().count(), SIDECAR_LOG_TAIL_LINES);
        assert!(tail.starts_with("line 5\n"));
        assert!(tail.ends_with(&format!("line {}", SIDECAR_LOG_TAIL_LINES + 4)));
        assert!(!tail.contains("line 4\n"));

        // Past the read window only the last lines survive, and the partial
        // first line the seek landed inside is dropped rather than shown.
        let padding = "x".repeat(SIDECAR_LOG_TAIL_BYTES as usize);
        fs::write(&path, format!("{padding}\nkept one\nkept two\n")).unwrap();
        assert_eq!(sidecar_log_tail(&path).unwrap(), "kept one\nkept two");

        let _ = fs::remove_dir_all(dir);
    }

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
