//! What the main WebView is allowed to become.
//!
//! The window loads the bundled start page and then the sidecar's own origin.
//! Anything else — a link in a model answer, a redirect from web content the
//! agent fetched — must not replace the application UI, because the window has
//! no address bar and no way back. Refused navigations are handed to the
//! browser by [`crate::opener`].

/// Loopback host the sidecar is pinned to by [`crate::overlay`].
const SIDECAR_HOST: &str = "127.0.0.1";

/// Whether the WebView may navigate to `url` itself.
///
/// # Parameters
/// - `url`: navigation target reported by the WebView.
///
/// # Returns
/// `true` for the bundled start page and the loopback sidecar, `false` for
/// everything else.
pub fn is_internal_url(url: &tauri::Url) -> bool {
    is_start_page(url)
        || matches!(
            (url.scheme(), url.host_str()),
            ("http" | "https", Some(SIDECAR_HOST))
        )
}

/// Whether `url` is the bundled start page, not the loopback sidecar.
///
/// # Parameters
/// - `url`: navigation target reported by the WebView.
///
/// # Returns
/// `true` for `tauri://` / `asset://` and `http(s)://tauri.localhost`.
pub fn is_start_page(url: &tauri::Url) -> bool {
    match url.scheme() {
        "tauri" | "asset" => true,
        "http" | "https" => url.host_str() == Some("tauri.localhost"),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(raw: &str) -> tauri::Url {
        tauri::Url::parse(raw).expect("test url")
    }

    #[test]
    fn allows_the_start_page_and_the_loopback_sidecar() {
        assert!(is_internal_url(&url("tauri://localhost/")));
        assert!(is_internal_url(&url("http://127.0.0.1:51010/")));
        assert!(is_internal_url(&url(
            "http://127.0.0.1:51010/session/1?x=2"
        )));
        assert!(is_internal_url(&url("http://tauri.localhost/index.html")));
        assert!(is_start_page(&url("tauri://localhost/")));
        assert!(is_start_page(&url("http://tauri.localhost/index.html")));
        assert!(!is_start_page(&url("http://127.0.0.1:51010/")));
    }

    #[test]
    fn refuses_web_content_and_other_loopback_spellings() {
        assert!(!is_internal_url(&url("https://deepseek.com/")));
        assert!(!is_internal_url(&url("http://127.0.0.1.example.test/")));
        // Only the pinned literal is internal: `localhost` and the IPv6 loopback
        // can resolve to a different listener than the one the shell spawned.
        assert!(!is_internal_url(&url("http://localhost:51010/")));
        assert!(!is_internal_url(&url("http://[::1]:51010/")));
        assert!(!is_internal_url(&url("file:///Users/me/secret.txt")));
        assert!(!is_internal_url(&url("javascript:alert(1)")));
    }
}
