//! Detect profile plugin changes the running sidecar has not picked up.
//!
//! `dsh plugin add` rewrites the profile manifest's `dsh.profile.bundles`, but
//! the running composition took that list once at boot: `composeLive` re-reads
//! only the patch files, so a newly installed plugin stays dark until the
//! sidecar starts again. The shell compares the manifest's modification time
//! against the stamp it took when this sidecar launched, which needs no
//! agreement with the sidecar about what "installed" means and no wall-clock
//! comparison.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Profile the desktop sidecar runs (`bin.js web`).
const DESKTOP_PROFILE: &str = "web";

/// Path of the profile manifest whose `dsh.profile.bundles` list the composition reads at boot.
///
/// # Parameters
/// - `home`: desktop `DSH_HOME`.
///
/// # Returns
/// The manifest path, whether or not it exists.
pub fn profile_manifest_path(home: &Path) -> PathBuf {
    home.join("profiles")
        .join(DESKTOP_PROFILE)
        .join("package.json")
}

/// Modification time of the profile manifest.
///
/// # Parameters
/// - `path`: manifest path from [`profile_manifest_path`].
///
/// # Returns
/// The stamp, or `None` when the profile has no manifest yet — a home that
/// never installed a plugin has nothing to be stale against.
pub fn manifest_stamp(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).ok()?.modified().ok()
}

/// Whether the profile manifest changed after this sidecar took its stamp.
///
/// # Parameters
/// - `booted`: stamp taken while launching the sidecar.
/// - `current`: stamp read now.
///
/// # Returns
/// `true` when the two differ. Inequality rather than ordering: an editor that
/// writes a older timestamp, or a restore from backup, still means the running
/// composition no longer matches the manifest on disk.
pub fn changed_since_boot(booted: Option<SystemTime>, current: Option<SystemTime>) -> bool {
    match (booted, current) {
        (None, None) => false,
        (a, b) => a != b,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn manifest_path_targets_the_desktop_profile() {
        let path = profile_manifest_path(Path::new("/home/.dsh"));
        assert!(path.ends_with("profiles/web/package.json"));
    }

    #[test]
    fn an_absent_manifest_is_never_stale() {
        let dir = std::env::temp_dir().join(format!("dsh-plugins-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = profile_manifest_path(&dir);
        assert_eq!(manifest_stamp(&path), None);
        assert!(!changed_since_boot(None, None));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_rewritten_manifest_reads_as_changed() {
        let dir = std::env::temp_dir().join(format!("dsh-plugins-rw-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let profile = dir.join("profiles").join(DESKTOP_PROFILE);
        fs::create_dir_all(&profile).unwrap();
        let path = profile_manifest_path(&dir);
        fs::write(&path, "{}").unwrap();
        let booted = manifest_stamp(&path);
        assert!(booted.is_some());
        assert!(!changed_since_boot(booted, manifest_stamp(&path)));

        // Set the stamp explicitly: two writes inside one filesystem timestamp
        // tick would otherwise compare equal and make this test flaky.
        let later = SystemTime::now() + Duration::from_secs(5);
        fs::File::open(&path).unwrap().set_modified(later).unwrap();
        assert!(changed_since_boot(booted, manifest_stamp(&path)));

        // Installing the first plugin into a home that had no manifest counts.
        assert!(changed_since_boot(None, manifest_stamp(&path)));
        let _ = fs::remove_dir_all(dir);
    }
}
