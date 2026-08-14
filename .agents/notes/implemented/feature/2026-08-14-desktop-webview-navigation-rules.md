# Agent Note: The desktop WebView keeps links out and file drops in

Status: implemented

English | [中文](2026-08-14-desktop-webview-navigation-rules.zh.md)

## Problem

The main window was declared entirely in `tauri.conf.json`, so it carried no navigation handlers, and three behaviors followed from the defaults.

A link in a model answer does nothing. The markdown renderer marks external links `target="_blank"`; WebKit asks the UI delegate for a new window, and `wry` returns none when no `on_new_window` handler is installed.

A plain external link replaces the application. Without `on_navigation`, any navigation loads in the main window, which has no address bar and no way back.

A file dropped on the conversation does nothing. `dragDropEnabled` defaults to on, and the handler `tauri-runtime-wry` installs returns `true` for every drag event, so `wry` never falls through to WebKit's own handling and the input bar's `drop` listener never fires.

## Decision

The window entry keeps its geometry in `tauri.conf.json` with `"create": false`, and `build_main_window` creates it from that entry so the handlers can be attached.

`navigation::is_internal_url` admits the bundled start page and the loopback sidecar by literal host; `localhost` and `[::1]` are refused because they can resolve to a listener this shell did not spawn. Everything else is refused and handed to `opener::open_external_url`, which launches only `http` and `https` — a refused navigation carries a URL from page content, and launching `file:` or an application scheme would let that content reach the disk or another application through the shell.

`dragDropEnabled` is off, which restores the page's own drop events.

## Alternatives considered

**Keep Tauri's drag-drop handler and forward the paths over IPC.** The capability does not cover the sidecar's remote origin, so the page cannot receive them without widening the permission surface.

**Add `tauri-plugin-opener`.** The shell already launches the file manager for the log directory; a plugin for one more launch adds a permission, a generated ACL, and a licence entry for about fifteen lines of code.

## Consequences

Links open in the browser and the application window stays on the application. Unit tests cover the internal-URL rules and the refused schemes. A launch under the launch daemon environment was verified to reach the sidecar page: WebKit's networking process held three connections to the sidecar port. Clicking a link is a manual check.
