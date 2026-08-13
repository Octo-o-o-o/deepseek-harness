# Agent Note: Desktop sidecar has one stop entry outside the state lock

Status: implemented

English | [中文](2026-08-14-desktop-sidecar-supervisor.zh.md)

## Problem

The Stage A shell stopped the sidecar from the tray, ctrlc, `RunEvent::Exit`, and `Drop` at once. Each path took the `Mutex` and called `shutdown` while still holding it, so a second stop waited on the same lock during the 5s grace. `LoggingLines` also appended post-ready stdout twice. `SIGKILL` of the shell left an orphan Node tree with no pid file for the next boot to reap.

## Decision

`SidecarSupervisor::request_stop` is the only public stop. It sets cancel, takes the `SidecarProcess` out of the mutex, then shuts down. A second call is a no-op. `SidecarProcess::shutdown` is idempotent; `Drop` only runs it when nobody already did. Reader threads are joined after the tree is reaped. Boot checks `is_cancelled` between steps and rolls back on navigate failure. `sidecar.pid` records the live Node pid; the next boot TERM/KILLs a leftover that still looks like `dsh web`.

## Alternatives considered

**Keep per-site shutdown and add a re-entrancy flag only on `SidecarProcess`.** That still leaves four call sites and lock-held waits.

**Supervise with a dedicated thread and a channel.** Correct, but heavier than taking the process out of the mutex.

## Consequences

Tray, ctrlc, and Exit share one path. Tests cover idempotent stop, ready-line garbage, HTTP 4MB cap, and pid-file bookkeeping. Windows Job Object assignment stays unverified on this machine.
