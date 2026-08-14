# Agent Note: Suppress the Windows console window next to the desktop shell

Status: implemented

English | [中文](2026-08-14-windows-desktop-console-window-suppression.zh.md)

## Problem

The release shell runs under the GUI subsystem (`windows_subsystem = "windows"` in `main.rs`), so it owns no console. Spawning the console-subsystem sidecar `node.exe` through `Command::spawn` without creation flags makes Windows allocate a fresh console for the child, and that console's window appears next to the app window on every launch of an installed copy. The stale-sidecar reaping path (`taskkill /PID … /T /F`) flashes the same kind of window whenever it fires.

## Decision

`process.rs` owns a `hide_child_console(&mut Command)` helper that applies the `CREATE_NO_WINDOW` creation flag (`0x0800_0000`) through `std::os::windows::process::CommandExt` on Windows and is a no-op elsewhere. The sidecar spawn and the `taskkill` reaping call apply it; the `ps` identity probe needs nothing because it is compiled `#[cfg(not(windows))]`. One flag at the sidecar covers the whole tree: `dsh-subprocess-local` never sets `detached` on Windows, so bash/pwsh and every grandchild inherit the sidecar's windowless console instead of allocating their own.

## Alternatives considered

**`DETACHED_PROCESS` instead of `CREATE_NO_WINDOW`.** Rejected because it leaves the child with no console at all, so a grandchild that wants a console must allocate a new (visible) one; `CREATE_NO_WINDOW` keeps a hidden console the tree can inherit, and it is the flag Node's own `windowsHide` maps to.

**`windowsHide: true` on every Node-side spawn.** Rejected as redundant: non-detached Windows children inherit the parent console, so hiding the sidecar's console hides the tree; the existing `windowsHide` sites (`dsh-directory-picker-native`, `dsh-native-command`) cover spawns whose parent may hold a visible console.

**Fixing only the sidecar spawn.** Rejected because reaping a stale sidecar after a crash would still flash a console window.

## Consequences

Installed Windows copies open no terminal window at startup or during stale-sidecar reaping. Debug builds keep their own console and lose nothing observable: sidecar stdio was already piped to `$DSH_HOME/logs/sidecar.log`, never to the parent console. A Windows-only test guards that the flag value stays a valid creation flag (an invalid value fails every child spawn), and a second one guards that `taskkill` still terminates its pid under the flag.
