# Agent Note: The sidebar brand row names the desktop application

Status: implemented

English | [中文](2026-08-14-desktop-brand-row.zh.md)

## Problem

The desktop shell serves the same web frontend the browser surface serves, so its sidebar opened with the web wordmark — the mark of a site, not of the application the person double-clicked. Nothing in the browser plane knew which of the two was running, and the frontend has no per-surface plugin configuration: client rows reach the browser through `window.__DSH_BOOT__`, which carries `id`/`url`/`rev`/`inject` and no `config`, so a cordis.yml value cannot select a form here.

## Decision

`ctx.connection` publishes `isDesktopShell`, a page fact derived from the bootstrap marker the shell's sidecar injects into the index it serves. It sits beside `isLoopback`: both are page-derived, fixed for the page's life, and readable before the first render, so no surface waits for the connection handshake or swaps form after it.

`ui-sidebar` injects `connection`, passes `desktopShell` through its slot inject face, and renders one of two brand forms. In the shell, the row is `AppMark` at 26px beside the product name split across two lines — `DeepSeek Harness` in the primary ink over `Desktop` in the secondary — because the one-line name does not fit the 300px column beside the mark without truncation. Everything else is unchanged: the row is still the New Session shortcut, and the collapsed rail is identical in both surfaces.

`AppMark` (ui-primitives) draws the packaged icon as vector: the whale knocked out in white on the icon's dark plate, plus a hairline ring so the plate's edge stays visible on a dark sidebar. It is the one primitive with literal colors rather than `--dsw-*` tokens — it identifies the installed application, which looks the same in the Dock under either theme. The whale path moved to `src/whale-path.ts`, shared with `FishLogo`.

## Alternatives considered

**Read the marker directly in ui-sidebar.** It skips the new dependency, but puts a third copy of the marker name in the tree (the web-app host half and the connection client half already own one each) and makes a UI package know a bootstrap detail.

**Extend `host.describe` with a shell field.** The description arrives only after the handshake, so the brand row would render the web wordmark first and swap; the fact is also the page's, not the host's — the same sidecar serves a browser tab.

**Add a `config` field to the client boot wire.** It is the composition-shaped answer, but the wire, its parser, and every consumer exist to carry code identity; one surface distinction does not justify widening them.

**Embed the icon PNG as a data URI.** The mark must render crisply at 26px in both themes and stay in step with the shipped icon; vector plus the shared whale path keeps one geometry source.

## Consequences

The desktop window opens with its own icon and full product name; browser tabs are untouched, as their `isDesktopShell` is false. Coverage: the marker read and its empty-string case, the handle field under both page states, the plugin's inject list and injected page fact, both brand forms in the shell component, and a slot-runtime snapshot of the desktop row. A future surface that must differ inside the packaged application reads the same flag rather than adding another channel.
