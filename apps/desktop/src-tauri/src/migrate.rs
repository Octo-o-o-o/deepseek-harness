//! First-launch copy from `~/.dsh` into the desktop home.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Directory names copied from a legacy `~/.dsh` tree.
pub const MIGRATE_DIRS: &[&str] = &[
    "sessions",
    "settings",
    "attachments",
    "storages",
    "profiles",
];

/// Names that must stay in the legacy home.
pub const SKIP_NAMES: &[&str] = &["credentials", ".credentials.yaml", ".credentials.yml"];

/// How far to run before injecting a test failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InjectFault {
    /// No injected failure.
    None,
    /// Fail after the per-item copy and before writing the marker.
    AfterCopy,
}

/// Outcome of a migrate attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationReport {
    /// Whether a copy actually ran.
    pub migrated: bool,
    /// Directory names copied from the legacy home.
    pub copied: Vec<String>,
    /// Directory names present in the legacy home but skipped.
    pub skipped: Vec<String>,
    /// Backup directory written under the new home, when one was created.
    pub backup: Option<PathBuf>,
}

/// Migration failure.
#[derive(Debug, thiserror::Error)]
pub enum MigrationError {
    /// Filesystem error while copying or rolling back.
    #[error("migration failed: {0}")]
    Io(#[from] io::Error),
    /// Injected failure used by tests.
    #[error("migration failed: injected fault after copy")]
    Injected,
}

/// Read the injected-fault knob from the process environment.
///
/// # Returns
/// [`InjectFault::AfterCopy`] when `DSH_DESKTOP_MIGRATE_FAIL` is `1`.
pub fn inject_fault_from_env() -> InjectFault {
    match std::env::var("DSH_DESKTOP_MIGRATE_FAIL") {
        Ok(value) if value == "1" => InjectFault::AfterCopy,
        _ => InjectFault::None,
    }
}

/// Legacy CLI home (`$HOME/.dsh`), overridable via `DSH_LEGACY_HOME`.
///
/// # Returns
/// Absolute path of the pre-desktop Harness home.
pub fn default_legacy_home() -> PathBuf {
    if let Ok(value) = std::env::var("DSH_LEGACY_HOME") {
        if !value.is_empty() {
            return PathBuf::from(value);
        }
    }
    crate::paths::user_home_dir().join(".dsh")
}

/// Whether the first-launch copy should run.
///
/// # Parameters
/// - `legacy`: `~/.dsh` (or test double).
/// - `home`: desktop `DSH_HOME`.
///
/// # Returns
/// `true` when the legacy tree exists and the new home has no marker.
pub fn should_migrate(legacy: &Path, home: &Path) -> bool {
    legacy.is_dir() && !home.join("migration-state.json").is_file()
}

/// Copy selected legacy directories into the desktop home.
///
/// Existing files in `home` are snapshotted to `migration-backup-<ts>` first.
/// A failure restores that snapshot and does not write the marker.
///
/// # Parameters
/// - `legacy`: source `~/.dsh`.
/// - `home`: destination desktop home.
/// - `fault`: test injection point.
///
/// # Returns
/// A report describing what was copied.
pub fn migrate_legacy_home(
    legacy: &Path,
    home: &Path,
    fault: InjectFault,
) -> Result<MigrationReport, MigrationError> {
    if !should_migrate(legacy, home) {
        return Ok(MigrationReport {
            migrated: false,
            copied: Vec::new(),
            skipped: Vec::new(),
            backup: None,
        });
    }
    fs::create_dir_all(home)?;
    let backup = snapshot_home(home)?;
    let mut copied = Vec::new();
    let mut skipped = Vec::new();
    let result = (|| -> Result<(), MigrationError> {
        for name in SKIP_NAMES {
            if legacy.join(name).exists() {
                skipped.push((*name).to_string());
            }
        }
        for name in MIGRATE_DIRS {
            let from = legacy.join(name);
            if !from.exists() {
                continue;
            }
            copy_tree(&from, &home.join(name))?;
            copied.push((*name).to_string());
        }
        if fault == InjectFault::AfterCopy {
            return Err(MigrationError::Injected);
        }
        let state = serde_json::json!({
            "status": "ok",
            "copied": copied,
            "skipped": skipped,
            "backup": backup.as_ref().map(|path| path.to_string_lossy().into_owned()),
        });
        fs::write(
            home.join("migration-state.json"),
            serde_json::to_vec_pretty(&state).map_err(io::Error::other)?,
        )?;
        Ok(())
    })();
    if let Err(error) = result {
        restore_snapshot(home, backup.as_deref())?;
        return Err(error);
    }
    Ok(MigrationReport {
        migrated: true,
        copied,
        skipped,
        backup,
    })
}

fn snapshot_home(home: &Path) -> io::Result<Option<PathBuf>> {
    let mut has_content = false;
    if let Ok(entries) = fs::read_dir(home) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name == "logs" || name == "desktop.lock" || name.starts_with("migration-backup-") {
                continue;
            }
            has_content = true;
            break;
        }
    }
    if !has_content {
        return Ok(None);
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    let backup = home.join(format!("migration-backup-{stamp}"));
    fs::create_dir_all(&backup)?;
    for entry in fs::read_dir(home)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str == "logs"
            || name_str == "desktop.lock"
            || name_str.starts_with("migration-backup-")
        {
            continue;
        }
        let dest = backup.join(&name);
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &dest)?;
        } else {
            fs::copy(entry.path(), dest)?;
        }
    }
    Ok(Some(backup))
}

fn restore_snapshot(home: &Path, backup: Option<&Path>) -> io::Result<()> {
    for name in MIGRATE_DIRS {
        let _ = fs::remove_dir_all(home.join(name));
    }
    let _ = fs::remove_file(home.join("migration-state.json"));
    let Some(backup) = backup else {
        return Ok(());
    };
    for entry in fs::read_dir(backup)? {
        let entry = entry?;
        let dest = home.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &dest)?;
        } else {
            fs::copy(entry.path(), dest)?;
        }
    }
    Ok(())
}

fn copy_tree(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let dest = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &dest)?;
        } else {
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(entry.path(), dest)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn temp_dir() -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "dsh-migrate-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn copies_data_dirs_and_skips_credentials() {
        let legacy = temp_dir();
        fs::create_dir_all(legacy.join("sessions")).unwrap();
        fs::write(legacy.join("sessions/a.jsonl"), "x\n").unwrap();
        fs::create_dir_all(legacy.join("credentials")).unwrap();
        fs::write(legacy.join("credentials/key"), "secret\n").unwrap();
        fs::write(legacy.join(".credentials.yaml"), "token: x\n").unwrap();
        let home = temp_dir();
        let report = migrate_legacy_home(&legacy, &home, InjectFault::None).unwrap();
        assert!(report.migrated);
        assert!(report.copied.contains(&"sessions".into()));
        assert!(report.skipped.contains(&"credentials".into()));
        assert!(home.join("sessions/a.jsonl").is_file());
        assert!(!home.join("credentials").exists());
        assert!(!home.join(".credentials.yaml").exists());
        assert!(home.join("migration-state.json").is_file());
        let again = migrate_legacy_home(&legacy, &home, InjectFault::None).unwrap();
        assert!(!again.migrated);
        let _ = fs::remove_dir_all(legacy);
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn injected_failure_rolls_back_to_backup() {
        let legacy = temp_dir();
        fs::create_dir_all(legacy.join("sessions")).unwrap();
        fs::write(legacy.join("sessions/a.jsonl"), "new\n").unwrap();
        let home = temp_dir();
        fs::write(home.join("keep.txt"), "old\n").unwrap();
        let error = migrate_legacy_home(&legacy, &home, InjectFault::AfterCopy).unwrap_err();
        assert!(matches!(error, MigrationError::Injected));
        assert!(!home.join("sessions").exists());
        assert!(!home.join("migration-state.json").exists());
        assert_eq!(fs::read_to_string(home.join("keep.txt")).unwrap(), "old\n");
        let _ = fs::remove_dir_all(legacy);
        let _ = fs::remove_dir_all(home);
    }
}
