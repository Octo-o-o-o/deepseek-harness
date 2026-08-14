//! Child environment whitelist. Credentials stay on the sidecar credentials path.

use std::process::Command;

/// Names copied from the parent. Everything else is dropped.
///
/// `DEEPSEEK_API_KEY` / `DEEPSEEK_BASE_URL` / `DEEPSEEK_SEARCH_BASE_URL` are the
/// official provider envs the sidecar already reads at boot. Other secrets stay
/// out of the child: they belong to `$DSH_HOME` credentials, not the process
/// environment. Locale, temp, home, PATH, and proxy vars are required for a
/// normal Node boot. `SystemRoot`/`SystemDrive` look Windows-only but are the
/// pair Node's crypto init needs: without `SystemRoot` the sidecar aborts
/// during `InitializeOncePerProcess` (`ncrypto::CSPRNG` assertion) before any
/// JS runs. libuv-based parents mask this by re-injecting `SystemRoot`
/// themselves; Rust's `Command` does not.
pub const INHERITED_ENV: &[&str] = &[
    "HOME",
    "USERPROFILE",
    "PATH",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "TMPDIR",
    "TEMP",
    "TMP",
    "SystemRoot",
    "SystemDrive",
    "DEEPSEEK_API_KEY",
    "DEEPSEEK_BASE_URL",
    "DEEPSEEK_SEARCH_BASE_URL",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "http_proxy",
    "https_proxy",
    "ALL_PROXY",
    "all_proxy",
    "NO_PROXY",
    "no_proxy",
];

/// Build the sidecar environment: whitelist plus caller extras (`DSH_HOME`, …).
///
/// # Parameters
/// - `extra`: pairs the shell always injects.
///
/// # Returns
/// The exact env the child will see.
pub fn resolved_sidecar_env(extra: &[(String, String)]) -> Vec<(String, String)> {
    let mut env = Vec::new();
    for key in INHERITED_ENV {
        if extra.iter().any(|(name, _)| name == key) {
            continue;
        }
        if let Ok(value) = std::env::var(key) {
            if !value.is_empty() {
                env.push(((*key).to_string(), value));
            }
        }
    }
    env.extend(extra.iter().cloned());
    env
}

/// Clear the child environment and apply [`resolved_sidecar_env`].
///
/// # Parameters
/// - `command`: sidecar `Command`.
/// - `extra`: pairs the shell always injects.
pub fn apply_sidecar_env(command: &mut Command, extra: &[(String, String)]) {
    command.env_clear();
    for (key, value) in resolved_sidecar_env(extra) {
        command.env(key, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extra_pairs_win_and_unknown_parent_vars_are_dropped() {
        let env = resolved_sidecar_env(&[("DSH_HOME".into(), "/tmp/dsh-home".into())]);
        assert!(env
            .iter()
            .any(|(key, value)| key == "DSH_HOME" && value == "/tmp/dsh-home"));
        assert!(env
            .iter()
            .all(|(key, _)| { INHERITED_ENV.contains(&key.as_str()) || key == "DSH_HOME" }));
    }
}
