# Agent Note: Desktop alignment onto dsh 0.1.0-rc.8

Status: implemented

English | [中文](2026-08-20-desktop-rc8-alignment.zh.md)

## Problem

This fork's desktop shell supervises `dsh web` and loads the sidecar's Web GUI. Upstream `0.1.0-rc.8` changed that composition: local `dsh web` opens the default browser unless `--no-open` is passed, the sidebar brand row became generic slots with a "DSH Local Build" fallback, and SQLite session files at schema 15 are refused at schema 17 with no migration. Leaving the fork on rc.7 would freeze the packaged GUI behind those product changes. Merging without adapting the desktop composition would open an unpaired browser tab beside the WebView, drop the packaged AppMark row, and treat `apps/desktop` as a publishable release member.

## Decision

The fork merge-forwards onto `dsh-v0.1.0-rc.8` and keeps the non-desktop surfaces unchanged when `DSH_DESKTOP_TOKEN` / `DSH_DESKTOP_BOOTSTRAP_NONCE` are unset.

Desktop composition never hands a browser the loopback URL. `handoffBrowser` is `config.openBrowser && !ssh && desktop === undefined`. The sidecar argv also pins `--no-open` (pack self-check included), so a forgotten code path still does not spawn `open`.

The sidebar shell is rc.8's slot model. The packaged mark and two-line name occupy `sidebar.brand.mark`, `sidebar.brand.name`, and `conversation.hero.brand.mark` from `@deepseek-ai/dsh-client-ui-plugin-restart`, which `apps/desktop/desktop.patch.yml` mounts. A browser tab on `dsh web` does not load that plugin, so it keeps the official occupants or the Local Build fallback.

`registerGuard`, desktop bootstrap, and the share gateway remain desktop-only paths in `dsh-web-app`. ArrowUp prompt recall stays in `InputBar` behind the rc.8 reference-chip key handling. Release-member directories stay `apps/(cli|web)` plus non-experimental packages, so `apps/desktop` remains private.

Default session persistence is still JSONL. An rc.7 SQLite file is incompatible with rc.8; this composition does not migrate it.

## Alternatives considered

**Keep `if (desktopShell)` inside `SidebarRoot`.** Allowed as a desktop-only execution path, but every later upstream brand-slot change would conflict on the same row. Occupants on the existing desktop-only plugin reuse rc.8's channel.

**Rely on `--no-open` alone, or on `handoffBrowser` alone.** Either forgets the other caller (pack self-check vs a hand-built tree). Both are required.

**Add a new `ui-brand-desktop` package.** A third client package for three slot registrations duplicates the desktop patch layer that already exists to keep desktop UI out of the shared web bundle.

**Widen `apps/[^/]+` in the release-member regex and depend on `private: true`.** rc.8 treats a private release member as an error. The directory must stay off the publish set.

## Consequences

A packaged launch no longer opens Safari/Edge beside the WebView. The brand row is an occupant, so a future official-profile desktop build would collide on the same single slots unless that profile stays off. Users who pointed the sqlite persistence plugin at `~/.dsh` cannot open those files in this build; JSONL users are unaffected. Payload-manifest `cli` must be regenerated against rc.8 before the next desktop pack.

## Testing

`packages/bundle/web-app/tests/web-app.spec.ts` asserts desktop bootstrap still registers, and that `internals.openBrowser` is not called when the paired desktop env is set. `packages/client/ui-plugin-restart/tests/browser-plugin.client.spec.tsx` asserts the three brand holes fill and dispose. `cargo test desktop_args_pin_loopback` pins `--no-open` on the sidecar argv. `scripts/check-workspace-constraints.spec.ts` rejects `apps/desktop` as a release member.
