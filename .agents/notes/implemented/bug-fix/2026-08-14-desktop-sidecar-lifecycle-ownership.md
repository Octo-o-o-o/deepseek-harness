# Agent Note: Desktop shutdown watches the process group and never joins on the main thread

Status: implemented

English | [中文](2026-08-14-desktop-sidecar-lifecycle-ownership.zh.md)

## Problem

Four lifecycle defects survived the [supervisor rework](2026-08-14-desktop-sidecar-supervisor.md).

`ChildTree::is_alive` asked only the leader. The sidecar starts bash, pwsh, and picker processes in its group, so a leader that exits on SIGTERM made the tree look dead: the forced kill was skipped and the unbounded join on the log readers then waited on pipe ends a surviving grandchild still held. Quit hung.

`RunEvent::Exit` ran `request_stop()` and then joined the boot thread — on the main thread. The boot thread polled `window.url()`, which posts to that same loop and blocks for the answer. Each waited for the other.

`SidecarSupervisor::install` read `stopping` outside the mutex. A stop landing between that read and the write took an empty slot and returned, after which `install` stored a process nobody would stop.

Nothing watched the sidecar after boot. It could exit and the window would keep showing a page whose server was gone.

## Decision

`is_alive` reaps the leader and then asks the group with `killpg(pgid, 0)`, treating only `ESRCH` as gone. Joining a log reader waits at most 2s and otherwise leaves the thread detached, because quitting must not depend on a grandchild closing a pipe.

`RunEvent::Exit` stops and returns; `AppState::join_boot` is gone. `install` and `request_stop` both take the mutex before reading or writing `stopping`, and both shut a process down outside the lock.

After the boot sequence reaches `Visible`, the boot thread stays on `wait_for_unexpected_exit`. An exit nobody asked for stops the shell, returns the boot error, and `show_error` navigates back to the bundled start page before writing the message, since the window is on the sidecar's origin by then and that origin is what just died.

## Alternatives considered

**A separate watchdog thread.** It would contend with the stop path for ownership of the child; the boot thread already owns the sequence and has nothing else to do after `Visible`.

**Automatic restart.** `boot_and_navigate` installs a panic hook, takes the directory lock, and runs the migration, so it is not re-entrant. Restarting in place needs a state machine with a bound, which is worth doing only once the error state exists.

## Consequences

Verified on a launch under the launch daemon environment: `kill -9` of the sidecar cleared `sidecar.pid` within 2s with the application still running and no orphan, and SIGTERM of the shell exited in about 1s with nothing left behind.
