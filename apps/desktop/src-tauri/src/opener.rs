//! Hand work to the platform: a directory to the file manager, a web URL to the
//! default browser. Every caller reaches the host through this module so the
//! per-platform launcher exists once.

use std::io;
use std::path::Path;
use std::process::Command;

/// Schemes the shell will hand to the browser. A denied navigation carries an
/// arbitrary URL from page content, so `file:`, `javascript:`, and custom
/// application schemes are never launched: opening those would let a page reach
/// the local disk or another application through the shell's own privileges.
const EXTERNAL_SCHEMES: &[&str] = &["http", "https"];

/// Whether [`open_external_url`] will launch this URL.
///
/// # Parameters
/// - `url`: candidate URL, typically from a refused navigation.
///
/// # Returns
/// `true` for the web schemes the browser handles.
pub fn is_openable_external(url: &tauri::Url) -> bool {
    EXTERNAL_SCHEMES.contains(&url.scheme())
}

/// Open a web URL in the user's default browser.
///
/// # Parameters
/// - `url`: URL a refused navigation or a new-window request carried.
///
/// # Returns
/// `Ok(())` once the launcher process started; `Ok(())` also when the scheme is
/// not web, because refusing to launch it is the intended outcome.
pub fn open_external_url(url: &tauri::Url) -> io::Result<()> {
    if !is_openable_external(url) {
        return Ok(());
    }
    open_command().arg(url.as_str()).spawn()?;
    Ok(())
}

/// Open a directory in the host file manager.
///
/// # Parameters
/// - `path`: directory to reveal.
///
/// # Returns
/// `Ok(())` once the launcher process started.
pub fn open_path(path: &Path) -> io::Result<()> {
    open_command().arg(path).spawn()?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn open_command() -> Command {
    Command::new("open")
}

#[cfg(target_os = "windows")]
fn open_command() -> Command {
    Command::new("explorer")
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn open_command() -> Command {
    Command::new("xdg-open")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(raw: &str) -> tauri::Url {
        tauri::Url::parse(raw).expect("test url")
    }

    #[test]
    fn web_schemes_are_openable() {
        assert!(is_openable_external(&url("https://deepseek.com/")));
        assert!(is_openable_external(&url("http://example.test/path?q=1")));
    }

    #[test]
    fn local_and_script_schemes_are_refused() {
        assert!(!is_openable_external(&url("file:///etc/passwd")));
        assert!(!is_openable_external(&url("javascript:alert(1)")));
        assert!(!is_openable_external(&url("data:text/html,<script>")));
        assert!(!is_openable_external(&url("vscode://file/etc/hosts")));
    }

    #[test]
    fn refused_scheme_starts_no_process() {
        assert!(open_external_url(&url("javascript:alert(1)")).is_ok());
    }
}
