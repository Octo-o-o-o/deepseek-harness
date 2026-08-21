# Agent Note: Desktop alignment onto dsh 0.1.1-rc.1

Status: implemented

English | [中文](2026-08-21-desktop-rc11-alignment.zh.md)

## Problem

This fork's desktop shell supervises `dsh web` and loads the sidecar's Web GUI. Upstream `0.1.1-rc.1` ships a vision catalog model, replaces ad-hoc index.html taps with a structured injection table plus `renderIndex`, and answers missing SPA paths with 404 instead of rewriting them to `index.html` at 200. Merging without keeping the desktop-only seats would drop `registerGuard`, ArrowUp prompt recall, and the `apps/(cli|web)` release-member regex.

The rc.8 overlay, brand patch, and `--no-open` argv remain required; that decision lives in [the rc.8 alignment note](2026-08-20-desktop-rc8-alignment.md).

## Decision

The fork merge-forwards onto `origin/master` at `0.1.1-rc.1` and keeps the non-desktop surfaces unchanged when `DSH_DESKTOP_TOKEN` / `DSH_DESKTOP_BOOTSTRAP_NONCE` are unset.

`registerGuard` still runs before every HTTP match and upgrade. `tapIndex` remains the escape hatch after `renderIndex` applies structured `IndexInjection` rows; the desktop bootstrap script still registers through `tapIndex`. ArrowUp prompt recall stays in `InputBar` beside the new edit-range bookkeeping. Release-member directories stay `apps/(cli|web)` plus non-experimental packages.

The packaged composition still disables `ui-brand-official` and pins sidecar argv to `dsh web --port 0 --host 127.0.0.1 --no-open`. Desktop health still requires `GET /` 200 with `__DSH_BOOT__`; the boot assignment is now `globalThis["__DSH_BOOT__"] = …`, which that substring still matches. SQLite `SCHEMA_VERSION` remains 17; `SESSION_FORMAT_VERSION` remains 0.

Vision support is the catalog row `deepseek-v4-flash-vision-exp` with `inputModalities: ['text', 'image']`. The desktop WebView and Tauri capabilities do not grow a new image path: `ui-attachment` already owns intake.

## Alternatives considered

**Take upstream `webserver` / `InputBar` / release-member regex wholesale.** That deletes the admission seat, ArrowUp recall, or treats `apps/desktop` as a publishable member. Each of those is a desktop-only obligation from the rc.8 merge.

**Move desktop bootstrap from `tapIndex` onto an `IndexInjection` row.** The nonce-gated script is markup no row expresses; `tapIndex` is the documented escape hatch after structured rows.

**Pin plugin peers to exact `0.1.1-rc.1` in the same change.** Desktop users and `dsh@next` users would then disagree. The deploy plugin dual-subscribes `credentials/updated` and `credentials/reference-updated` instead.

## Consequences

The fork-only `@deepseek-ai/dsh-client-ui-plugin-restart` package stays on the dsh release-family version line so `scripts/release/pack.ts` can pack the sidecar. After pack, `payload-manifest.json` `cli` is `0.1.1-rc.1`. Sidecar pack, desktop CI, and the signed macOS release keep running `pnpm run build:official`.

## Testing

`packages/bundle/web-app/tests/web-app.spec.ts` asserts desktop bootstrap still registers through `tapIndex`/`renderIndex`, and that `internals.openBrowser` is not called when the paired desktop env is set. `packages/host/webserver/tests/webserver.spec.ts` covers both the admission guard and fresh `webserver/index-inject` collection. `packages/client/ui-conversation/tests/input-bar.client.spec.tsx` covers ArrowUp recall together with edit ranges. `apps/desktop/tests/desktop-patch.spec.ts` asserts `ui-brand-official` is disabled. `cargo test overlay` refuses argv that omit `--no-open`. `scripts/check-workspace-constraints.spec.ts` rejects `apps/desktop` as a release member. Sidecar pack records `payload-manifest.json` `cli` as `0.1.1-rc.1` and self-check requires a ready line, `GET /` 200 with `__DSH_BOOT__`, and a clean SIGTERM.
