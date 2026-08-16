//! Resolve Node, the web CLI entry, DSH_HOME, and the sidecar working directory.

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

/// Path resolution failure.
#[derive(Debug, thiserror::Error)]
pub enum PathError {
    /// `node` is not on PATH and no bundled runtime was found.
    #[error("node runtime not found")]
    NodeNotFound,
    /// `apps/cli/lib/bin.js` is missing from the checkout and the bundle.
    #[error("dsh web entry not found")]
    WebBinNotFound,
}

/// Default `DSH_HOME` for the desktop app (overridden by the `DSH_HOME` env).
///
/// # Returns
/// `~/.dsh`, the directory the npm CLI uses, or the env override.
pub fn default_dsh_home() -> PathBuf {
    if let Ok(value) = env::var("DSH_HOME") {
        if !value.is_empty() {
            return PathBuf::from(value);
        }
    }
    platform_dsh_home()
}

/// User home directory (`HOME` / `USERPROFILE`).
///
/// # Returns
/// The process home, or `.` when neither env is set.
pub fn user_home_dir() -> PathBuf {
    home_dir()
}

/// The dsh ecosystem home ($HOME/.dsh), shared with the npm/npx CLI so
/// sessions, settings, and workspaces are live-shared between the desktop
/// app and "npx @deepseek-ai/dsh". DSH_HOME env still overrides everything.
fn platform_dsh_home() -> PathBuf {
    home_dir().join(".dsh")
}

/// Default sidecar cwd: last workspace from `desktop-state.json`, else `~/Documents`.
///
/// Stage D owns persistence of that file. Stage A uses the same default so the
/// agent never treats the application install directory as a workspace.
/// A recorded workspace that no longer exists falls back to `~/Documents`:
/// spawning into a deleted directory would fail the whole boot.
///
/// # Parameters
/// - `home`: desktop `DSH_HOME`.
///
/// # Returns
/// A directory path to use as the sidecar `cwd`.
pub fn default_workspace_cwd(home: &Path) -> PathBuf {
    if let Ok(value) = env::var("DSH_WORKSPACE") {
        if !value.is_empty() {
            return PathBuf::from(value);
        }
    }
    let state_path = home.join("desktop-state.json");
    if let Ok(text) = std::fs::read_to_string(&state_path) {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
            if let Some(workspace) = value.get("workspace").and_then(|item| item.as_str()) {
                if !workspace.is_empty() {
                    let recorded = PathBuf::from(workspace);
                    if recorded.is_dir() {
                        return recorded;
                    }
                }
            }
        }
    }
    fallback_workspace()
}

fn fallback_workspace() -> PathBuf {
    let documents = home_dir().join("Documents");
    if documents.is_dir() {
        documents
    } else {
        home_dir()
    }
}

/// Persist the sidecar workspace when `desktop-state.json` is missing.
/// A `DSH_WORKSPACE` override is never persisted: the debug knob must not
/// become the permanent workspace of later launches.
///
/// # Parameters
/// - `home`: desktop `DSH_HOME`.
/// - `workspace`: directory chosen as the sidecar cwd.
///
/// # Returns
/// `Ok(())` after writing or when the file already exists or is overridden.
pub fn ensure_desktop_state(home: &Path, workspace: &Path) -> std::io::Result<()> {
    if env::var("DSH_WORKSPACE").is_ok_and(|value| !value.is_empty()) {
        return Ok(());
    }
    let path = home.join("desktop-state.json");
    if path.is_file() {
        return Ok(());
    }
    let body = serde_json::json!({ "workspace": workspace });
    fs::write(
        path,
        serde_json::to_vec_pretty(&body).map_err(io::Error::other)?,
    )
}

/// Nearby / Tailscale switches remembered in `desktop-state.json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SharePrefs {
    /// Nearby listen should come back after a relaunch.
    pub nearby: bool,
    /// Foreground Tailscale Serve should come back after a relaunch.
    pub tailscale: bool,
}

/// Read `{ nearby, tailscale }` from `desktop-state.json`.
///
/// Missing files, invalid JSON, and non-boolean fields all yield `false`.
/// This function never writes.
///
/// # Parameters
/// - `home`: desktop `DSH_HOME`.
///
/// # Returns
/// The recorded switches, or both `false` when they cannot be read.
pub fn read_share_prefs(home: &Path) -> SharePrefs {
    let default = SharePrefs {
        nearby: false,
        tailscale: false,
    };
    let Ok(text) = fs::read_to_string(home.join("desktop-state.json")) else {
        return default;
    };
    let Ok(value) = serde_json::from_str::<Value>(&text) else {
        return default;
    };
    SharePrefs {
        nearby: value
            .get("nearby")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        tailscale: value
            .get("tailscale")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    }
}

/// Merge `{ nearby, tailscale }` into `desktop-state.json` without dropping
/// `workspace` or unknown fields. Invalid JSON is left untouched.
///
/// # Parameters
/// - `home`: desktop `DSH_HOME`.
/// - `prefs`: switches to persist after the gateway / Serve reached that state.
///
/// # Returns
/// `Ok(())` after the atomic replace. `Err` when the file exists but is not a
/// JSON object — the caller must not treat that as a successful write.
pub fn merge_share_prefs(home: &Path, prefs: SharePrefs) -> io::Result<()> {
    let path = home.join("desktop-state.json");
    let mut value = match fs::read_to_string(&path) {
        Ok(text) => match serde_json::from_str::<Value>(&text) {
            Ok(Value::Object(map)) => Value::Object(map),
            Ok(_) => return Err(io::Error::other("desktop-state.json is not an object")),
            Err(_) => return Err(io::Error::other("desktop-state.json is not valid JSON")),
        },
        Err(err) if err.kind() == io::ErrorKind::NotFound => Value::Object(Map::new()),
        Err(err) => return Err(err),
    };
    let Some(object) = value.as_object_mut() else {
        return Err(io::Error::other("desktop-state.json is not an object"));
    };
    object.insert("nearby".into(), Value::Bool(prefs.nearby));
    object.insert("tailscale".into(), Value::Bool(prefs.tailscale));
    atomic_write_json(&path, &value)
}

/// Write `value` via a sibling temp file and rename over `path`.
fn atomic_write_json(path: &Path, value: &Value) -> io::Result<()> {
    let parent = path.parent().unwrap_or(Path::new("."));
    let tmp = parent.join(format!(
        ".{}.tmp-{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("desktop-state.json"),
        std::process::id()
    ));
    let mut bytes = serde_json::to_vec_pretty(value).map_err(io::Error::other)?;
    bytes.push(b'\n');
    fs::write(&tmp, bytes)?;
    replace_file(&tmp, path)
}

/// Replace `to` with `from`. Windows cannot rename over an existing file.
fn replace_file(from: &Path, to: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        if to.exists() {
            fs::remove_file(to)?;
        }
    }
    fs::rename(from, to)
}

/// Locate `node`: `DSH_NODE_PATH`, bundled runtime, then PATH.
///
/// # Parameters
/// - `exe`: current executable, used to find `.app/Contents/Resources`.
///
/// # Returns
/// Absolute path to a Node binary.
pub fn resolve_node(exe: &Path) -> Result<PathBuf, PathError> {
    if let Ok(value) = env::var("DSH_NODE_PATH") {
        if !value.is_empty() {
            return Ok(PathBuf::from(value));
        }
    }
    if let Some(bundled) = bundled_node(exe) {
        if bundled.is_file() {
            return Ok(bundled);
        }
    }
    which("node").ok_or(PathError::NodeNotFound)
}

/// Locate `apps/cli/lib/bin.js`: env, bundled resources, then repo walk.
///
/// # Parameters
/// - `exe`: current executable.
/// - `cwd`: process working directory (additional walk root).
///
/// # Returns
/// Absolute path to the built CLI entry.
pub fn resolve_web_bin(exe: &Path, cwd: &Path) -> Result<PathBuf, PathError> {
    if let Ok(value) = env::var("DSH_WEB_BIN") {
        if !value.is_empty() {
            return Ok(PathBuf::from(value));
        }
    }
    if let Some(bundled) = bundled_web_bin(exe) {
        if bundled.is_file() {
            return Ok(bundled);
        }
    }
    for start in [exe.to_path_buf(), cwd.to_path_buf()] {
        if let Some(root) = find_repo_root(&start) {
            let bin = root.join("apps/cli/lib/bin.js");
            if bin.is_file() {
                return Ok(bin);
            }
        }
    }
    Err(PathError::WebBinNotFound)
}

/// Walk parents looking for `pnpm-workspace.yaml` plus the built CLI entry.
///
/// # Parameters
/// - `start`: file or directory to walk upward from.
///
/// # Returns
/// Repository root when the checkout markers exist.
pub fn find_repo_root(start: &Path) -> Option<PathBuf> {
    let mut dir = if start.is_file() {
        start.parent()?.to_path_buf()
    } else {
        start.to_path_buf()
    };
    loop {
        if dir.join("pnpm-workspace.yaml").is_file() && dir.join("apps/cli/lib/bin.js").is_file() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// `.app/Contents/Resources` when `exe` lives in `Contents/MacOS`.
///
/// # Parameters
/// - `exe`: current executable.
///
/// # Returns
/// Resource directory for a packaged macOS app.
pub fn macos_resource_dir(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    if macos.file_name()?.to_str()? != "MacOS" {
        return None;
    }
    Some(macos.parent()?.join("Resources"))
}

fn bundled_node(exe: &Path) -> Option<PathBuf> {
    resource_dirs(exe).into_iter().find_map(|resources| {
        [
            resources.join("bin/node"),
            resources.join("bin/node.exe"),
            resources.join("sidecar/dist/bin/node"),
            resources.join("_up_/sidecar/dist/bin/node"),
        ]
        .into_iter()
        .find(|candidate| candidate.is_file())
    })
}

fn bundled_web_bin(exe: &Path) -> Option<PathBuf> {
    resource_dirs(exe).into_iter().find_map(|resources| {
        [
            resources.join("app/lib/bin.js"),
            resources.join("sidecar/dist/app/lib/bin.js"),
            resources.join("_up_/sidecar/dist/app/lib/bin.js"),
        ]
        .into_iter()
        .find(|candidate| candidate.is_file())
    })
}

/// Locate the desktop-only patch layer shipped beside the sidecar payload.
///
/// The layer activates rows that belong to the packaged application rather
/// than to `npx @deepseek-ai/dsh web`, so a missing file is not an error: a
/// source checkout that never packaged one still boots, just without those
/// rows.
///
/// # Parameters
/// - `exe`: the running executable's path.
/// - `cwd`: process working directory, used by the source-checkout fallback.
///
/// # Returns
/// The patch path when one exists.
pub fn resolve_desktop_patch(exe: &Path, cwd: &Path) -> Option<PathBuf> {
    for resources in resource_dirs(exe) {
        let candidate = resources.join("desktop.patch.yml");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    for start in [exe.to_path_buf(), cwd.to_path_buf()] {
        if let Some(root) = find_repo_root(&start) {
            let candidate = root.join("apps/desktop/desktop.patch.yml");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn resource_dirs(exe: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(macos) = macos_resource_dir(exe) {
        dirs.push(macos);
    }
    if let Some(parent) = exe.parent() {
        dirs.push(parent.to_path_buf());
        dirs.push(parent.join("resources"));
    }
    dirs
}

fn home_dir() -> PathBuf {
    env::var("HOME")
        .or_else(|_| env::var("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

fn which(name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    for dir in env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
        #[cfg(windows)]
        {
            let exe = dir.join(format!("{name}.exe"));
            if exe.is_file() {
                return Some(exe);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = env::temp_dir().join(format!(
            "dsh-desktop-paths-{nanos}-{}",
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn workspace_cwd_reads_desktop_state() {
        let home = temp_dir();
        let workspace = home.join("ws");
        fs::create_dir_all(&workspace).unwrap();
        fs::write(
            home.join("desktop-state.json"),
            format!(
                r#"{{"workspace":{}}}"#,
                serde_json::to_string(&workspace.to_string_lossy()).unwrap()
            ),
        )
        .unwrap();
        assert_eq!(default_workspace_cwd(&home), workspace);
        let _ = fs::remove_dir_all(home);
    }

    /// A recorded workspace that was deleted must not fail the boot: the
    /// fallback keeps spawning possible.
    #[test]
    fn workspace_cwd_falls_back_when_recorded_dir_is_gone() {
        let home = temp_dir();
        fs::write(
            home.join("desktop-state.json"),
            r#"{"workspace":"/tmp/dsh-definitely-missing-workspace"}"#,
        )
        .unwrap();
        let resolved = default_workspace_cwd(&home);
        assert_ne!(
            resolved,
            PathBuf::from("/tmp/dsh-definitely-missing-workspace")
        );
        assert!(resolved.is_dir());
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn repo_root_walk_finds_checkout_markers() {
        let root = temp_dir();
        fs::create_dir_all(root.join("apps/cli/lib")).unwrap();
        fs::write(root.join("pnpm-workspace.yaml"), "packages: []\n").unwrap();
        fs::write(root.join("apps/cli/lib/bin.js"), "#!/usr/bin/env node\n").unwrap();
        let nested = root.join("apps/desktop/src-tauri");
        fs::create_dir_all(&nested).unwrap();
        assert_eq!(find_repo_root(&nested), Some(root.clone()));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn writes_desktop_state_once() {
        let home = temp_dir();
        let workspace = PathBuf::from("/tmp/dsh-workspace");
        ensure_desktop_state(&home, &workspace).unwrap();
        ensure_desktop_state(&home, Path::new("/tmp/other")).unwrap();
        let text = fs::read_to_string(home.join("desktop-state.json")).unwrap();
        assert!(text.contains("/tmp/dsh-workspace"));
        assert!(!text.contains("/tmp/other"));
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn merge_share_prefs_keeps_workspace_and_unknown_fields() {
        let home = temp_dir();
        fs::write(
            home.join("desktop-state.json"),
            r#"{"workspace":"/tmp/ws","extra":1}"#,
        )
        .unwrap();
        merge_share_prefs(
            &home,
            SharePrefs {
                nearby: true,
                tailscale: false,
            },
        )
        .unwrap();
        let value: Value =
            serde_json::from_str(&fs::read_to_string(home.join("desktop-state.json")).unwrap())
                .unwrap();
        assert_eq!(value["workspace"], "/tmp/ws");
        assert_eq!(value["extra"], 1);
        assert_eq!(value["nearby"], true);
        assert_eq!(value["tailscale"], false);
        assert_eq!(
            read_share_prefs(&home),
            SharePrefs {
                nearby: true,
                tailscale: false
            }
        );
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn merge_share_prefs_refuses_to_clobber_invalid_json() {
        let home = temp_dir();
        let path = home.join("desktop-state.json");
        fs::write(&path, "not-json").unwrap();
        assert!(merge_share_prefs(
            &home,
            SharePrefs {
                nearby: true,
                tailscale: true
            }
        )
        .is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "not-json");
        fs::write(&path, "[1]").unwrap();
        assert!(merge_share_prefs(
            &home,
            SharePrefs {
                nearby: true,
                tailscale: true
            }
        )
        .is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "[1]");
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn read_share_prefs_defaults_when_the_file_is_missing() {
        let home = temp_dir();
        assert_eq!(
            read_share_prefs(&home),
            SharePrefs {
                nearby: false,
                tailscale: false
            }
        );
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn bundled_runtime_reads_resources_bin() {
        let root = temp_dir();
        let macos = root.join("DeepSeek.app/Contents/MacOS");
        let resources = root.join("DeepSeek.app/Contents/Resources");
        fs::create_dir_all(resources.join("bin")).unwrap();
        fs::create_dir_all(resources.join("app/lib")).unwrap();
        fs::write(resources.join("bin/node"), "node\n").unwrap();
        fs::write(resources.join("app/lib/bin.js"), "bin\n").unwrap();
        let exe = macos.join("dshd");
        fs::create_dir_all(&macos).unwrap();
        fs::write(&exe, "exe\n").unwrap();
        assert_eq!(bundled_node(&exe), Some(resources.join("bin/node")));
        assert_eq!(
            bundled_web_bin(&exe),
            Some(resources.join("app/lib/bin.js"))
        );
        let _ = fs::remove_dir_all(root);
    }
}
