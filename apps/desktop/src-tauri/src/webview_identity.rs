//! Safari.app identity the macOS WebView presents to the page.
//!
//! System WKWebView reports Apple's vendor and an AppleWebKit token, but no
//! `Version/… Safari/…` form. The composer recovery is gated on that form
//! ([`isSafariBrowser`] in `packages/client/ui-conversation`). Widening the
//! detector would change every Apple web view, including `dsh web` embeddings
//! this fork must not touch. The main window therefore sets this User-Agent
//! on macOS only.
//!
//! See `.agents/notes/implemented/bug-fix/2026-08-17-desktop-wkwebview-safari-identity.md`.

/// User-Agent that makes `isSafariBrowser` accept this WKWebView.
///
/// The AppleWebKit token is the one system WKWebView reports on this host.
/// `Version/26.5 Safari/605.1.15` is the Safari.app form the detector
/// requires. Keep this string byte-identical to the matching fixture in
/// `packages/client/ui-conversation/tests/safari.client.spec.ts`. Windows
/// WebView2 is Chromium and does not use this value.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) const MACOS_SAFARI_WEBVIEW_USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.5 Safari/605.1.15";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_agent_carries_safari_app_version_and_safari_tokens() {
        let ua = MACOS_SAFARI_WEBVIEW_USER_AGENT;
        let version = ua.find("Version/").expect("Version/");
        let safari = ua.find("Safari/").expect("Safari/");
        assert!(safari > version);
        assert!(ua.as_bytes()[version + "Version/".len()].is_ascii_digit());
        assert!(ua.as_bytes()[safari + "Safari/".len()].is_ascii_digit());
        assert!(ua.contains("AppleWebKit/605.1.15"));
        assert!(!ua.contains("CriOS"));
        assert!(!ua.contains("EdgiOS"));
        assert!(!ua.contains("FxiOS"));
        assert!(!ua.contains("OPiOS"));
    }
}
