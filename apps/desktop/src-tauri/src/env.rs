//! Child environment whitelist. Credentials stay on the sidecar credentials path.

use std::collections::BTreeMap;
use std::process::Command;

/// Names copied from the parent. Everything else is dropped.
///
/// `DEEPSEEK_API_KEY` / `DEEPSEEK_BASE_URL` / `DEEPSEEK_SEARCH_BASE_URL` are the
/// official provider envs the sidecar already reads at boot. Other secrets stay
/// out of the child: they belong to `$DSH_HOME` credentials, not the process
/// environment. Locale, temp, home, PATH, and proxy vars are required for a
/// normal Node boot.
///
/// The rest serve the agent's own child processes: `SSH_AUTH_SOCK` is what makes
/// `git push` over SSH work, `SHELL` / `USER` / `LOGNAME` are the identity facts
/// command-line tools read, and the certificate variables are how a network with
/// its own certificate authority is trusted by both Node and the tools it runs.
pub const INHERITED_ENV: &[&str] = &[
    "HOME",
    "USERPROFILE",
    "PATH",
    "SHELL",
    "USER",
    "LOGNAME",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "TMPDIR",
    "TEMP",
    "TMP",
    "SSH_AUTH_SOCK",
    "DEEPSEEK_API_KEY",
    "DEEPSEEK_BASE_URL",
    "DEEPSEEK_SEARCH_BASE_URL",
    "NODE_EXTRA_CA_CERTS",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "http_proxy",
    "https_proxy",
    "ALL_PROXY",
    "all_proxy",
    "NO_PROXY",
    "no_proxy",
];

/// The one name the login shell overrides rather than fills in.
///
/// Every other whitelisted name keeps the launch value when there is one: it was
/// set by whoever started the application and is therefore deliberate. `PATH` is
/// the exception because the launch value is not a choice — an application
/// opened from the Dock receives the launch daemon's `PATH`, which names none of
/// the tools the user installed.
const LOGIN_SHELL_OVERRIDES: &[&str] = &["PATH"];

/// Merge the three environment sources under the whitelist.
///
/// # Parameters
/// - `launch`: value of each name in this process, as launched.
/// - `login`: what the user's login shell exports ([`crate::shell_env`]).
/// - `extra`: pairs the shell always injects, which always win.
///
/// # Returns
/// The exact env the child will see, holding only whitelisted names plus `extra`.
pub fn merge_sidecar_env(
    launch: &BTreeMap<String, String>,
    login: &BTreeMap<String, String>,
    extra: &[(String, String)],
) -> Vec<(String, String)> {
    let mut env = Vec::new();
    for key in INHERITED_ENV {
        if extra.iter().any(|(name, _)| name == key) {
            continue;
        }
        let (first, second) = if LOGIN_SHELL_OVERRIDES.contains(key) {
            (login.get(*key), launch.get(*key))
        } else {
            (launch.get(*key), login.get(*key))
        };
        let value = first
            .filter(|value| !value.is_empty())
            .or(second)
            .filter(|value| !value.is_empty());
        if let Some(value) = value {
            env.push(((*key).to_string(), value.clone()));
        }
    }
    env.extend(extra.iter().cloned());
    env
}

/// Build the sidecar environment: whitelist plus caller extras (`DSH_HOME`, …).
///
/// # Parameters
/// - `login`: what the user's login shell exports; empty keeps the launch env.
/// - `extra`: pairs the shell always injects.
///
/// # Returns
/// The exact env the child will see.
pub fn resolved_sidecar_env(
    login: &BTreeMap<String, String>,
    extra: &[(String, String)],
) -> Vec<(String, String)> {
    let launch = INHERITED_ENV
        .iter()
        .filter_map(|key| {
            std::env::var(key)
                .ok()
                .map(|value| ((*key).to_string(), value))
        })
        .collect();
    merge_sidecar_env(&launch, login, extra)
}

/// Clear the child environment and apply [`resolved_sidecar_env`].
///
/// # Parameters
/// - `command`: sidecar `Command`.
/// - `login`: what the user's login shell exports.
/// - `extra`: pairs the shell always injects.
pub fn apply_sidecar_env(
    command: &mut Command,
    login: &BTreeMap<String, String>,
    extra: &[(String, String)],
) {
    command.env_clear();
    for (key, value) in resolved_sidecar_env(login, extra) {
        command.env(key, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect()
    }

    fn value_of<'a>(env: &'a [(String, String)], key: &str) -> Option<&'a str> {
        env.iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
    }

    #[test]
    fn extra_pairs_win_and_unknown_parent_vars_are_dropped() {
        let env = resolved_sidecar_env(
            &BTreeMap::new(),
            &[("DSH_HOME".into(), "/tmp/dsh-home".into())],
        );
        assert!(env
            .iter()
            .any(|(key, value)| key == "DSH_HOME" && value == "/tmp/dsh-home"));
        assert!(env
            .iter()
            .all(|(key, _)| { INHERITED_ENV.contains(&key.as_str()) || key == "DSH_HOME" }));
    }

    #[test]
    fn the_login_shell_path_replaces_the_launch_path() {
        let env = merge_sidecar_env(
            &map(&[("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")]),
            &map(&[("PATH", "/opt/homebrew/bin:/usr/bin")]),
            &[],
        );
        assert_eq!(value_of(&env, "PATH"), Some("/opt/homebrew/bin:/usr/bin"));
    }

    #[test]
    fn other_launch_values_survive_the_login_shell() {
        let env = merge_sidecar_env(
            &map(&[("DEEPSEEK_API_KEY", "from-launch")]),
            &map(&[
                ("DEEPSEEK_API_KEY", "from-profile"),
                ("LANG", "en_US.UTF-8"),
            ]),
            &[],
        );
        assert_eq!(value_of(&env, "DEEPSEEK_API_KEY"), Some("from-launch"));
        assert_eq!(value_of(&env, "LANG"), Some("en_US.UTF-8"));
    }

    #[test]
    fn the_login_shell_cannot_widen_the_whitelist() {
        let env = merge_sidecar_env(
            &BTreeMap::new(),
            &map(&[
                ("AWS_SECRET_ACCESS_KEY", "leak"),
                ("PATH", "/opt/homebrew/bin"),
            ]),
            &[],
        );
        assert_eq!(value_of(&env, "AWS_SECRET_ACCESS_KEY"), None);
        assert_eq!(value_of(&env, "PATH"), Some("/opt/homebrew/bin"));
    }

    #[test]
    fn an_empty_login_shell_answer_keeps_the_launch_path() {
        let env = merge_sidecar_env(&map(&[("PATH", "/usr/bin")]), &BTreeMap::new(), &[]);
        assert_eq!(value_of(&env, "PATH"), Some("/usr/bin"));
    }
}
