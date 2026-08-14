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
        use std::os::windows::io::AsRawHandle;
        // share_mode(0) rejects every other open of the lock file: a second
        // desktop (intended) but also the sidecar's chokidar `fs.watch`, whose
        // EBUSY then kills Node before the web app boots. Open fully shared
        // and hold exclusivity with a non-blocking byte-range lock instead —
        // a second desktop still fails with ERROR_LOCK_VIOLATION, while
        // watchers, scanners, and editors can open the file harmlessly.
        const FILE_SHARE_READ: u32 = 0x0000_0001;
        const FILE_SHARE_WRITE: u32 = 0x0000_0002;
        const FILE_SHARE_DELETE: u32 = 0x0000_0004;
        const LOCKFILE_FAIL_IMMEDIATELY: u32 = 0x0000_0001;
        const LOCKFILE_EXCLUSIVE_LOCK: u32 = 0x0000_0002;
        #[repr(C)]
        struct Overlapped {
            internal: usize,
            internal_high: usize,
            offset: u32,
            offset_high: u32,
            event: *mut core::ffi::c_void,
        }
        extern "system" {
            fn LockFileEx(
                file: *mut core::ffi::c_void,
                flags: u32,
                reserved: u32,
                length_low: u32,
                length_high: u32,
                overlapped: *mut Overlapped,
            ) -> i32;
        }
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .open(&path)
            .map_err(|error| {
                if error.raw_os_error() == Some(32) {
                    LockError::Busy
                } else {
                    LockError::Io(error)
                }
            })?;
        let overlapped = Overlapped {
            internal: 0,
            internal_high: 0,
            offset: 0,
            offset_high: 0,
            event: core::ptr::null_mut(),
        };
        let locked = unsafe {
            LockFileEx(
                file.as_raw_handle().cast(),
                LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
                0,
                1,
                0,
                (&overlapped as *const Overlapped).cast_mut(),
            )
        };
        if locked == 0 {
            let error = io::Error::last_os_error();
            return Err(if error.raw_os_error() == Some(33) {
                LockError::Busy
            } else {
                LockError::Io(error)
            });
        }
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
