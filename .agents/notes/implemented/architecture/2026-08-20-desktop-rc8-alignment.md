# Agent Note: Desktop alignment onto dsh 0.1.0-rc.8

Status: implemented

English | [中文](2026-08-20-desktop-rc8-alignment.zh.md)

## Problem

This fork's desktop shell supervises `dsh web` and loads the sidecar's Web GUI. Upstream `0.1.0-rc.8` changed that composition: local `dsh web` opens the default browser unless `--no-open` is passed, the sidebar brand row became generic slots with a "DSH Local Build" fallback, and SQLite session files at schema 15 are refused at schema 17 with no migration. Leaving the fork on rc.7 would freeze the packaged GUI behind those product changes. Merging without adapting the desktop composition would open an unpaired browser tab beside the WebView, drop the packaged AppMark row, and treat `apps/desktop` as a publishable release member.

## Decision

The fork merge-forwards onto `dsh-v0.1.0-rc.8` and keeps the non-desktop surfaces unchanged when `DSH_DESKTOP_TOKEN` / `DSH_DESKTOP_BOOTSTRAP_NONCE` are unset.

Desktop composition never hands a browser the loopback URL. `handoffBrowser` is `config.openBrowser && !ssh && desktop === undefined`. The sidecar argv also pins `--no-open` (pack self-check included), so a forgotten code path still does not spawn `open`.

The sidebar shell is rc.8's slot model. The packaged mark and two-line name occupy `sidebar.brand.mark`, `sidebar.brand.name`, and `conversation.hero.brand.mark` from `@deepseek-ai/dsh-client-ui-plugin-restart`, which `apps/desktop/desktop.patch.yml` mounts. That overlay also sets `ui-brand-official` to `disabled: true`: those holes are `single` slots, and the official plugin fills them whenever client artifacts were built with `DSH_CLIENT_BUILD_PROFILE=official`. A browser tab on `dsh web` does not load the desktop patch, so it keeps the official occupants or the Local Build fallback.

`registerGuard`, desktop bootstrap, and the share gateway remain desktop-only paths in `dsh-web-app`. ArrowUp prompt recall stays in `InputBar` behind the rc.8 reference-chip key handling. Release-member directories stay `apps/(cli|web)` plus non-experimental packages, so `apps/desktop` remains private. The sidecar overlay requires `--no-open` the same way it requires `--host 127.0.0.1`.

Default session persistence is still JSONL. An rc.7 SQLite file is incompatible with rc.8; this composition does not migrate it.

## Alternatives considered

**Keep `if (desktopShell)` inside `SidebarRoot`.** Allowed as a desktop-only execution path, but every later upstream brand-slot change would conflict on the same row. Occupants on the existing desktop-only plugin reuse rc.8's channel.

**Rely on `--no-open` alone, or on `handoffBrowser` alone.** Either forgets the other caller (pack self-check vs a hand-built tree). Both are required. The overlay's `--no-open` requirement is the spawn-time check that neither caller can skip.

**Add a new `ui-brand-desktop` package.** A third client package for three slot registrations duplicates the desktop patch layer that already exists to keep desktop UI out of the shared web bundle.

**Widen `apps/[^/]+` in the release-member regex and depend on `private: true`.** rc.8 treats a private release member as an error. The directory must stay off the publish set.

## Consequences

A packaged launch no longer opens Safari/Edge beside the WebView. Disabling `ui-brand-official` in the desktop patch keeps the packaged mark even when client artifacts were built with the official profile; a browser `dsh web` is unchanged. Users who pointed the sqlite persistence plugin at `~/.dsh` cannot open those files in this build; JSONL users are unaffected. Sidecar pack, desktop CI, and the signed macOS release all run `pnpm run build:official` because `scripts/release/pack.ts` refuses any other client artifact profile.

## Testing

`packages/bundle/web-app/tests/web-app.spec.ts` asserts desktop bootstrap still registers, and that `internals.openBrowser` is not called when the paired desktop env is set. `packages/client/ui-plugin-restart/tests/browser-plugin.client.spec.tsx` asserts the three brand holes fill and dispose. `apps/desktop/tests/desktop-patch.spec.ts` asserts `ui-brand-official` is disabled. `cargo test desktop_args_pin_loopback` pins `--no-open` on the sidecar argv; `cargo test overlay` refuses argv that omit it. `scripts/check-workspace-constraints.spec.ts` rejects `apps/desktop` as a release member. `scripts/release/desktop-mac.spec.ts` pins the signed release to `pnpm run build:official`. The current sidecar `cli` version is recorded in [the 0.1.1-rc.1 alignment note](2026-08-21-desktop-rc11-alignment.md).
