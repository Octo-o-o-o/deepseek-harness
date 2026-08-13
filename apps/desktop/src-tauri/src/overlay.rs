//! Desktop security overlay: fail-closed on non-loopback bind and trust expansion.

/// Overlay rejection.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum OverlayError {
    /// Sidecar args would bind or advertise a non-loopback host.
    #[error("desktop overlay refused a non-loopback sidecar bind")]
    NonLoopbackBind,
    /// Sidecar args would expand the /api trusted-host fence.
    #[error("desktop overlay refused a trusted-host expansion")]
    TrustedHostExpansion,
    /// Sidecar args omitted the required loopback host flag.
    #[error("desktop overlay requires --host 127.0.0.1")]
    MissingLoopbackHost,
}

/// Whether `host` is the loopback literal the desktop composition allows.
///
/// # Parameters
/// - `host`: a `--host` value.
///
/// # Returns
/// `true` only for `127.0.0.1`.
pub fn is_allowed_bind_host(host: &str) -> bool {
    host == "127.0.0.1"
}

/// Reject sidecar argv that would expose the loopback API or widen trust.
///
/// # Parameters
/// - `args`: full sidecar argument list after the node program (script + flags).
///
/// # Returns
/// `Ok(())` when the argv is the desktop-safe `dsh web --port 0 --host 127.0.0.1` family.
pub fn assert_safe_sidecar_args(args: &[String]) -> Result<(), OverlayError> {
    let mut saw_loopback_host = false;
    let mut index = 0;
    while index < args.len() {
        let current = args[index].as_str();
        if current == "--trusted-host" || current.starts_with("--trusted-host=") {
            return Err(OverlayError::TrustedHostExpansion);
        }
        if let Some(value) = flag_value(args, index, "--host") {
            if !is_allowed_bind_host(value) {
                return Err(OverlayError::NonLoopbackBind);
            }
            saw_loopback_host = true;
        }
        if current == "0.0.0.0" {
            return Err(OverlayError::NonLoopbackBind);
        }
        index += 1;
    }
    if saw_loopback_host {
        Ok(())
    } else {
        Err(OverlayError::MissingLoopbackHost)
    }
}

fn flag_value<'a>(args: &'a [String], index: usize, flag: &str) -> Option<&'a str> {
    let current = args[index].as_str();
    if current == flag {
        return args.get(index + 1).map(String::as_str);
    }
    current
        .strip_prefix(flag)
        .and_then(|rest| rest.strip_prefix('='))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|part| (*part).to_string()).collect()
    }

    #[test]
    fn accepts_loopback_web_argv() {
        assert_eq!(
            assert_safe_sidecar_args(&args(&[
                "apps/cli/lib/bin.js",
                "web",
                "--port",
                "0",
                "--host",
                "127.0.0.1",
            ])),
            Ok(())
        );
    }

    #[test]
    fn rejects_all_interfaces_and_trust_expansion() {
        assert_eq!(
            assert_safe_sidecar_args(&args(&["web", "--port", "0", "--host", "0.0.0.0"])),
            Err(OverlayError::NonLoopbackBind)
        );
        assert_eq!(
            assert_safe_sidecar_args(&args(&[
                "web",
                "--host",
                "127.0.0.1",
                "--trusted-host",
                "evil.example",
            ])),
            Err(OverlayError::TrustedHostExpansion)
        );
        assert_eq!(
            assert_safe_sidecar_args(&args(&["web", "--port", "0"])),
            Err(OverlayError::MissingLoopbackHost)
        );
    }
}
