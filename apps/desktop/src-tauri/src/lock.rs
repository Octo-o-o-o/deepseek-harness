//! Exclusive lock on the desktop data directory.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

/// Held exclusive lock. Dropping the file releases the lock.
#[derive(Debug)]
pub struct HomeLock {
    _file: File,
}

/// Lock acquisition failure.
#[derive(Debug, thiserror::Error)]
pub enum LockError {
    /// Another process already holds the lock.
    #[error("another dshd instance is using the data directory")]
    Busy,
    /// The lock file could not be created.
    #[error("failed to lock data directory: {0}")]
    Io(#[from] io::Error),
}

/// Try to take an exclusive, process-held lock under `home`.
///
/// # Parameters
/// - `home`: desktop `DSH_HOME`.
///
/// # Returns
/// A guard that must be stored for the process lifetime.
pub fn try_lock_home(home: &Path) -> Result<HomeLock, LockError> {
    std::fs::create_dir_all(home)?;
    let path = home.join("desktop.lock");
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // Not locally verified on this machine. share_mode(0) is exclusive.
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .share_mode(0)
            .open(&path)
            .map_err(|error| {
                if error.raw_os_error() == Some(32) {
                    LockError::Busy
                } else {
                    LockError::Io(error)
                }
            })?;
        return Ok(HomeLock { _file: file });
    }
    #[cfg(not(windows))]
    {
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&path)?;
        lock_exclusive(&file)?;
        Ok(HomeLock { _file: file })
    }
}

#[cfg(unix)]
fn lock_exclusive(file: &File) -> Result<(), LockError> {
    use std::os::unix::io::AsRawFd;
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if rc == 0 {
        Ok(())
    } else {
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::WouldBlock {
            Err(LockError::Busy)
        } else {
            Err(LockError::Io(error))
        }
    }
}

#[cfg(not(any(unix, windows)))]
fn lock_exclusive(_file: &File) -> Result<(), LockError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn temp_dir() -> std::path::PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "dsh-lock-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn second_lock_is_busy() {
        let home = temp_dir();
        let first = try_lock_home(&home).unwrap();
        let second = try_lock_home(&home).unwrap_err();
        assert!(matches!(second, LockError::Busy));
        drop(first);
        let third = try_lock_home(&home).unwrap();
        drop(third);
        let _ = std::fs::remove_dir_all(home);
    }
}
