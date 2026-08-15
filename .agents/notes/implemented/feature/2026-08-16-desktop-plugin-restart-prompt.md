# Agent Note: Offer a restart when installed plugins are not composed

Status: implemented

English | [中文](2026-08-16-desktop-plugin-restart-prompt.zh.md)

## Problem

Installing a plugin into a profile leaves the running desktop application unchanged, with nothing on screen saying so. `dsh plugin add` rewrites the profile manifest's `dsh.profile.bundles`, but the composition read that list once at boot: `composeLive` re-reads the patch files on every change and nothing else, so the bundle layer is a startup snapshot. The plugin is installed, its UI never appears, and the only remedy — quit and reopen — is not stated anywhere.

## Decision

The sidebar foot grows an entry, visible only while the profile manifest differs from what the running sidecar composed, that restarts the application after a confirmation.

**Hot reload was ruled out, not deferred.** Three independent obstacles each defeat it on their own:

- The client HMR receiver refuses an entry it does not already hold: `reload()` looks the id up in the loader tree and returns after a warning when it is absent (`packages/client/hmr/src/client/index.ts`). It swaps bundles for plugins already mounted; it does not admit new ones. Its `graph` frame is explicitly unused.
- `window.__DSH_BOOT__` is injected into the served index, and the browser loader tree is built from it, so a new entry needs a fresh document.
- Recomposing the host tree needs the root Include entry, which lives in a `WeakMap` private to `dsh-app-boot`. Reaching it would mean widening a shared package's public surface for one desktop consumer.

**Detection belongs to the shell.** It stamps the profile manifest's modification time while launching the sidecar and compares it on demand. That needs no agreement with the sidecar about what "installed" means, no wall-clock comparison, and no host-side plugin. Inequality rather than ordering: an editor writing an older timestamp, or a restore from backup, still means the composition no longer matches the disk.

**The page reaches the shell through a runtime capability.** The sidecar page is a remote origin to Tauri and reaches no command until a capability names it. The port is OS-assigned, so the shell registers the capability once the port is known (`dynamic-acl`), naming that exact origin and exactly two commands. A wildcard port in the static capability would have handed those commands to any local process that can bind one — the same loopback-carries-no-identity problem the bootstrap token exists to solve.

**The confirmation is unconditional.** A restart stops the whole local session process, and the browser has no cross-session view of which sessions hold an answer in flight: `SessionListState` carries ids, summaries, phase, and jobs, and no running flag. The dialog states the cost — an answer in progress is interrupted, saved history is not — rather than guessing whether one exists.

**The plugin is activated by the shell's own patch layer.** `apps/desktop/desktop.patch.yml` is passed as `--patch`, so the row never reaches `npx @deepseek-ai/dsh web`, and this fork's diff against upstream stays in files upstream does not have. One upstream line is unavoidable: the package must be a dependency of `@deepseek-ai/dsh` to enter the sidecar payload at all. It is inert there — nothing composes it without the patch layer.

## Alternatives considered

**A wildcard remote capability (`http://127.0.0.1:*`).** Simpler, and it needs no feature flag. But Tauri's own tests cover hostname globbing only, with no case for a wildcard port, so the semantics would have been assumed rather than known — and the grant would cover every port on loopback rather than this launch's.

**Detect in the sidecar and report over an `/api` method or a dedicated RPC channel.** The sidecar cannot see its own staleness without re-reading and re-composing the manifest, which is the work being avoided; and it cannot restart the application either. Both halves of the fact are the shell's.

**Watch the manifest and push an event instead of polling.** The shell has no event channel to the page. Building one for a five-second question is more moving parts than the question deserves.

**Extend `composeLive` to re-read the bundle list.** It lives in `apps/cli`, which every surface runs, so it would change what `dsh web` does on a manifest write — and the client half still could not mount the new entry, so the plugin would remain invisible.

**Put the entry in the tray instead of the sidebar.** No new package, no capability, no IPC. Rejected because the person installing a plugin is looking at the window, not the menu bar.

## Verification

`pnpm vitest run packages/client/ui-plugin-restart` — 13 tests across two files, no uncovered lines or branches in the package.

The suites assert what the design promises rather than what the code does: the bridge answers "nothing pending" outside the shell, on a failing command, and on any answer that is not exactly `true`, so an unreachable shell can never pin a banner to the sidebar; the entry stays invisible until the shell reports a change; clicking it opens the dialog without restarting; cancel closes it with no command sent; only the dialog's accept reaches `restart_for_plugins`; a rejected restart reports the failure and leaves the dialog open; a change landing after mount is picked up by the poll, whose interval dies with the component; and answers arriving after unmount set no state. Registration and dictionary both ride the plugin fiber, checked by disposing it.

Shell side: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test` pass, the last covering the manifest-path shape, an absent manifest never reading as stale, and a rewritten manifest reading as changed (stamp set explicitly, since two writes inside one filesystem timestamp tick would compare equal).

Two shell tests, `lock::tests::second_lock_is_busy` and `health::tests::host_describe_accepts_a_chunked_reply`, fail intermittently under load — different one each run, each passing alone and on rerun. They predate this change and are unrelated to it.

## Consequences

A person who installs a plugin now learns that a restart is needed, and gets it in one click instead of quitting and reopening. What they do not get is the plugin without a restart; the three obstacles above would each have to be lifted for that, and the first two are properties of how the browser half boots.

The desktop shell now has a command surface reachable from the sidecar page, which it did not before. It is two commands wide, scoped to one origin that changes every launch, and granted after the port is known. `notify_attention` remains unreachable — it was never granted a capability — so adding it later is a deliberate act rather than a side effect of this one.

The poll asks every five seconds for the life of the window. It is one IPC round trip to a process on the same machine, and it stops with the component.
