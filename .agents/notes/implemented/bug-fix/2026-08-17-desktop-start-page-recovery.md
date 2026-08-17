# Agent Note: Start page recovers a dead sidecar and refuses a live swipe-back

Status: implemented

English | [中文](2026-08-17-desktop-start-page-recovery.zh.md)

## Problem

After the sidecar dies or boot fails, the start page showed the log tail and an Open-log-directory control. The only way back to a running host was Quit from the tray and launch again. Separately, WKWebView history can return to the bundled start page (`tauri://localhost` or `http://tauri.localhost`) while the sidecar is still alive. That page then sits on “Starting the local host…” forever, because its splash hook does not navigate again.

## Decision

The start page error state offers **Restart local host**, which invokes the existing `restart_for_plugins` command. Default capability now grants that command to the main window's start page; the sidecar origin still receives it only through the per-launch remote capability. A full process restart releases the home lock and respawns the sidecar.

Once the WebView has navigated to the sidecar, `AppState` stores the loopback origin without the bootstrap nonce fragment — before the client-ready wait, so a swipe during that window cannot restore the splash. `on_navigation` refuses a start-page URL while that origin is set and navigates back to it. `request_stop` clears the origin so a real failure can return to the start page. An update `install` that fails after that stop navigates back to the start page and shows the same Restart control.

## Alternatives considered

**Re-enter `boot_and_navigate` on the same process.** Rejected for this change: the boot thread, home lock, and supervisor assume a single pass. Process restart is the recovery the plugin-restart path already owns.

**Leave swipe-back as a documented limit.** Rejected: the start page then lies about a host that is already running.

**Re-navigate with the original `#dshd-nonce=` fragment.** The nonce is single-use. After bootstrap the cookie is enough; the stored origin is `http://127.0.0.1:<port>/`.

## Consequences

A failed boot or an unexpected sidecar exit can recover from the start page without a tray Quit. A live session cannot be replaced by the splash through history back. The restart still kills an in-flight turn, same as plugin restart.

## Testing

`cargo test` covers `is_start_page` versus the loopback sidecar, updater `install_verified_update` ordering, and that a failed install has already stopped the sidecar. Not covered: a GUI swipe-back in a packaged WebView, clicking Restart on a live error splash, or an `install()` failure returning to that splash.
