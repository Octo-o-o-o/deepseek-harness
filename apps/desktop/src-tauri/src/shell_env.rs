//! Read the user's login-shell environment.
//!
//! An application started from the Dock, Finder, or Spotlight inherits the
//! launch daemon's environment, whose `PATH` is `/usr/bin:/bin:/usr/sbin:/sbin`.
//! The same harness started from a terminal sees the shell's `PATH`, so the
//! agent's `bash` tool finds Homebrew, nvm, and the user's own scripts there and
//! not here. This module asks the login shell what it exports so the desktop
//! session matches the terminal one.
//!
//! The shell's answer is a value source, not a permission: only the names in
//! [`crate::env::INHERITED_ENV`] reach the sidecar, so a credential exported in
//! `.zshrc` for an unrelated service stays out of the agent's environment.

use std::collections::BTreeMap;
#[cfg(unix)]
use std::process::{Command, Stdio};
#[cfg(unix)]
use std::time::{Duration, Instant};

/// How long the login shell may take before the launch environment is kept.
#[cfg(unix)]
pub const RESOLVE_TIMEOUT: Duration = Duration::from_secs(5);

/// Marker printed around the environment dump so shell profile output that
/// writes to stdout cannot be read as an environment entry.
#[cfg(unix)]
const MARK: &str = "__DSH_ENV__";

/// Environment variable set for the child so a profile can detect this probe
/// and skip work meant for an interactive session.
#[cfg(unix)]
const PROBE_MARKER: &str = "DSH_RESOLVING_ENVIRONMENT";

/// Environment variable that turns the probe off.
const DISABLE_VAR: &str = "DSH_DESKTOP_SHELL_ENV";

/// Whether the probe is enabled.
///
/// # Parameters
/// - `disable`: value of `DSH_DESKTOP_SHELL_ENV`, if set.
///
/// # Returns
/// `false` only for the explicit off values.
pub fn probe_enabled(disable: Option<&str>) -> bool {
    !matches!(disable, Some("0") | Some("false") | Some("off"))
}

/// The argument vector that makes a POSIX shell print its exported environment.
///
/// The shell runs as an interactive login shell because `PATH` additions live
/// in `.zshrc` and `.bash_profile` alike, and only one of those is read by a
/// non-interactive shell.
///
/// # Returns
/// Arguments after the shell program.
#[cfg(unix)]
pub fn login_shell_args() -> Vec<String> {
    vec![
        "-ilc".to_string(),
        format!("printf '%s' {MARK}; env -0; printf '%s' {MARK}"),
    ]
}

/// Parse the `env -0` block between the markers.
///
/// # Parameters
/// - `stdout`: raw child stdout, which may carry profile output around the block.
///
/// # Returns
/// The exported pairs, or an empty map when the block is absent or unterminated.
#[cfg(unix)]
pub fn parse_env_block(stdout: &[u8]) -> BTreeMap<String, String> {
    let text = String::from_utf8_lossy(stdout);
    let mut parts = text.split(MARK);
    let (Some(_before), Some(block), Some(_after)) = (parts.next(), parts.next(), parts.next())
    else {
        return BTreeMap::new();
    };
    let mut entries = BTreeMap::new();
    for record in block.split('\0') {
        if record.is_empty() {
            continue;
        }
        let Some((name, value)) = record.split_once('=') else {
            continue;
        };
        if name.is_empty() {
            continue;
        }
        entries.insert(name.to_string(), value.to_string());
    }
    entries
}

/// Ask the login shell for its exported environment.
///
/// A shell that is missing, fails, or does not answer within
/// [`RESOLVE_TIMEOUT`] yields an empty map: the caller then keeps the launch
/// environment, which is the pre-existing behavior rather than a boot failure.
///
/// # Parameters
/// - `shell`: login shell program, normally `$SHELL`.
///
/// # Returns
/// The exported pairs the shell reported.
#[cfg(unix)]
pub fn resolve_login_shell_env(shell: &str) -> BTreeMap<String, String> {
    let spawned = Command::new(shell)
        .args(login_shell_args())
        .env(PROBE_MARKER, "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let Ok(mut child) = spawned else {
        return BTreeMap::new();
    };
    let deadline = Instant::now() + RESOLVE_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {}
            Err(_) => return BTreeMap::new(),
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            eprintln!(
                "desktop: login shell environment probe timed out; keeping the launch environment"
            );
            return BTreeMap::new();
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let Ok(output) = child.wait_with_output() else {
        return BTreeMap::new();
    };
    parse_env_block(&output.stdout)
}

/// Resolve the login-shell environment for the current user, honoring
/// [`DISABLE_VAR`].
///
/// # Returns
/// The exported pairs, empty when the probe is disabled, unsupported, or failed.
pub fn login_shell_env() -> BTreeMap<String, String> {
    if !probe_enabled(std::env::var(DISABLE_VAR).ok().as_deref()) {
        return BTreeMap::new();
    }
    // Windows GUI processes inherit the user environment, so there is nothing
    // for a shell to add there.
    #[cfg(unix)]
    {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
        resolve_login_shell_env(&shell)
    }
    #[cfg(not(unix))]
    {
        BTreeMap::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn block(body: &str) -> Vec<u8> {
        format!("profile noise\n{MARK}{body}{MARK}").into_bytes()
    }

    #[cfg(unix)]
    #[test]
    fn reads_pairs_between_the_markers() {
        let parsed = parse_env_block(&block(
            "PATH=/opt/homebrew/bin:/usr/bin\0LANG=en_US.UTF-8\0",
        ));
        assert_eq!(
            parsed.get("PATH").map(String::as_str),
            Some("/opt/homebrew/bin:/usr/bin")
        );
        assert_eq!(parsed.get("LANG").map(String::as_str), Some("en_US.UTF-8"));
    }

    #[cfg(unix)]
    #[test]
    fn keeps_values_that_contain_separators() {
        let parsed = parse_env_block(&block("SCRIPT=a=b\nc\0"));
        assert_eq!(parsed.get("SCRIPT").map(String::as_str), Some("a=b\nc"));
    }

    #[cfg(unix)]
    #[test]
    fn profile_output_outside_the_markers_is_not_an_entry() {
        let parsed = parse_env_block(&block("PATH=/usr/bin\0"));
        assert_eq!(parsed.len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn an_unterminated_block_yields_nothing() {
        assert!(parse_env_block(format!("{MARK}PATH=/usr/bin\0").as_bytes()).is_empty());
        assert!(parse_env_block(b"no markers here").is_empty());
    }

    #[test]
    fn the_probe_is_opt_out() {
        assert!(probe_enabled(None));
        assert!(probe_enabled(Some("1")));
        assert!(!probe_enabled(Some("0")));
        assert!(!probe_enabled(Some("false")));
    }

    #[cfg(unix)]
    #[test]
    fn a_missing_shell_yields_nothing() {
        assert!(resolve_login_shell_env("/nonexistent/shell/for/tests").is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn a_real_shell_reports_its_path() {
        let resolved = resolve_login_shell_env("/bin/sh");
        assert!(resolved.contains_key("PATH"), "resolved: {resolved:?}");
    }
}
