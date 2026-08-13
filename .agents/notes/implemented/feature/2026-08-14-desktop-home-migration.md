# Agent Note: Desktop home, first-launch migration, and a process lock

Status: implemented

English | [中文](2026-08-14-desktop-home-migration.zh.md)

## Problem

`dsh web` defaults to `~/.dsh`. A packaged desktop app that kept that path would fight the CLI for the same writer and would store sessions next to the user's shell config. The desktop also needed a one-time copy of existing CLI data, a way to refuse a second writer, and a log location that survives a sidecar crash.

## Decision

The shell injects `DSH_HOME` as `~/Library/Application Support/DeepSeekHarness` on macOS and `%APPDATA%\DeepSeekHarness` on Windows, overridable by the env. First launch copies `sessions`, `settings`, `attachments`, `storages`, and `profiles` from `~/.dsh` (or `DSH_LEGACY_HOME`) when the new home has no `migration-state.json`. Credentials are not copied. Existing files in the new home are snapshotted to `migration-backup-<ts>` first; `DSH_DESKTOP_MIGRATE_FAIL=1` or any copy error restores that snapshot and skips the marker. `desktop.lock` is an exclusive `flock` (Windows: `share_mode(0)`, not locally verified). `desktop-state.json` remembers the sidecar cwd. `logs/sidecar.log` rotates at 50MB; panics append to `logs/crash.log`.

## Alternatives considered

**Share `~/.dsh` with the CLI and skip migration.** That keeps one tree, but two writers and a hidden-dot directory inside a packaged app were rejected in the desktop proposal.

**Move instead of copy.** A failed move would destroy the CLI home; copy-plus-marker leaves `~/.dsh` intact.

## Consequences

A second desktop process sees the Chinese busy-home error and does not spawn a sidecar. Tests cover the copy, the credential skip, injected rollback, lock contention, log rotation, and `desktop-state.json`. Opening the log directory from the error page uses a Tauri command when the WebView can invoke it.
