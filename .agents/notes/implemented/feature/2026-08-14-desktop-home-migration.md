# Agent Note: Desktop home, first-launch migration, and a process lock

Status: implemented

English | [中文](2026-08-14-desktop-home-migration.zh.md)

## Problem

`dsh web` defaults to `~/.dsh`. An earlier desktop build gave the shell its own platform app-data home to avoid two writers; that split the user's sessions, settings, and workspaces into two trees that never converged, so the same work looked different depending on which entry point started it. The desktop still needs a one-time copy out of that abandoned home, a way to refuse a second desktop writer, and a log location that survives a sidecar crash.

## Decision

The shell injects `DSH_HOME` as `~/.dsh` — the CLI's own home — overridable by the env, so a desktop window and `npx @deepseek-ai/dsh web` read and write one tree and may run at the same time, each on its own OS-assigned port.

First launch copies `sessions`, `settings`, `attachments`, `storages`, and `profiles` out of the pre-unification desktop home (`~/Library/Application Support/DeepSeekHarness`, `%APPDATA%\DeepSeekHarness`, or `DSH_LEGACY_HOME`) only when `~/.dsh` has no `migration-state.json` **and** holds none of those directories: a home with real CLI data is authoritative and is never overwritten. Credentials are not copied. Existing files in the home are snapshotted to `migration-backup-<ts>` first; `DSH_DESKTOP_MIGRATE_FAIL=1` or any copy error restores that snapshot and skips the marker.

`desktop.lock` is an exclusive `flock` (Windows: `share_mode(0)`, not locally verified) taken by the shell alone, so it excludes a second `dshd` without excluding a CLI server. `sidecar.pid` records the sidecar's process id **and** the entry script it was launched with, and the next boot reaps that process only when the live command still names that script: in a shared home a pid alone would, after pid reuse, authorize killing a `dsh web` the user started themselves. `desktop-state.json` remembers the sidecar cwd. `logs/sidecar.log` rotates at 50MB; panics append to `logs/crash.log`.

## Alternatives considered

**Keep the separate platform app-data home.** It removes concurrent writers by construction, but the two trees never converge and the product has no story for moving work between them.

**Move instead of copy out of the legacy home.** A failed move would destroy data; copy-plus-marker leaves the legacy tree intact for a manual retry.

**Serialize the CLI and the desktop on one home lock.** Whichever started first would win and the other would refuse to start, which is exactly the coexistence the unified home exists to provide.

## Consequences

A second desktop process sees the Chinese busy-home error and does not spawn a sidecar; a CLI `dsh web` is unaffected and shares sessions live. Tests cover the copy, the credential skip, the refusal to migrate over existing product data, injected rollback, lock contention, log rotation, `desktop-state.json`, and the reap refusal for a foreign process at the recorded pid. Opening the log directory from the error page uses a Tauri command when the WebView can invoke it.
