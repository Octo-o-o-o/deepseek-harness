# Agent Note: Desktop WKWebView presents Safari.app identity

Status: implemented

English | [中文](2026-08-17-desktop-wkwebview-safari-identity.zh.md)

## Problem

The conversation composer recovers Safari's stale native textarea layout only when `isSafariBrowser` sees Apple's vendor and the `Version/… Safari/…` user-agent form ([textarea recovery](2026-08-13-safari-textarea-soft-wrap-reflow.md)). macOS dshd hosts that GUI in WKWebView. The system WebView reports `vendor = Apple Computer, Inc.` and an AppleWebKit token, but no `Version/` or `Safari/` tokens, so the recovery never runs. The same Backspace-across-soft-wrap defect the recovery exists to clear is a WebKit text-control bug, and WKWebView is that engine.

## Decision

The macOS main window sets a pinned User-Agent that keeps the measured AppleWebKit token and adds Safari.app's `Version/26.5 Safari/605.1.15` form. `WebviewWindowBuilder::user_agent` applies it before the window is built. `isSafariBrowser` is unchanged: an unadorned Apple web view still returns false, and every non-desktop composition keeps the identity its host actually has.

The string lives in `apps/desktop/src-tauri/src/webview_identity.rs` as `MACOS_SAFARI_WEBVIEW_USER_AGENT` and is byte-identical to the matching fixture in `packages/client/ui-conversation/tests/safari.client.spec.ts`. Windows WebView2 is Chromium and does not receive this value.

## Alternatives considered

**Widen `isSafariBrowser` to every Apple web view.** Rejected: that function is shared. Treating any `vendor === Apple` WebKit view as Safari would change `dsh web` embeddings this fork must not touch, and the existing "Apple web view" fixture exists specifically to keep that case false.

**Spoof `navigator.userAgent` from `desktop-bootstrap.ts`.** The bootstrap script already runs only when the desktop env is set, so the fork rule would hold. It still races the first composer layout against script injection, and it lies to the page about a field the WebView already owns. Native `user_agent` is the field the detector reads.

**Leave the detector false and accept the caret bug in dshd.** Rejected: RC7 shipped the recovery for this engine family, and the desktop host is WKWebView.

## Consequences

macOS dshd presents the Safari.app token form, so a native shortening that violates the textarea overflow invariant pays the same four forced layouts as Safari.app. `dsh web` in Safari.app is already true; `dsh web` in Chrome stays false. An unadorned WKWebView outside this shell remains excluded. The recovery still does not run unless the native-shrink signal and the overflow mismatch are both present.

## Testing

`cargo test webview_identity` asserts the pinned string carries `Version/<digit>` before `Safari/<digit>` and none of the alternate iOS browser tokens. `pnpm exec vitest run packages/client/ui-conversation/tests/safari.client.spec.ts` covers the unadorned macOS WKWebView (false) and the pinned token form (true). A host WKWebView constructed with this `customUserAgent` reports the same `navigator.userAgent` and still `vendor = Apple Computer, Inc.`, which `isSafariBrowser` accepts.

Not covered: driving Backspace across a soft-wrap threshold inside a live dshd window. That path needs GUI automation that steals focus; the component recovery tests and the identity gate stand in until an automatable Safari/WKWebView lane exists.
