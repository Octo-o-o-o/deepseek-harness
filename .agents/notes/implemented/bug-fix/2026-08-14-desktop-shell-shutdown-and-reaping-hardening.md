# Agent Note: Harden desktop-shell shutdown, stale-sidecar reaping, and boot-failure visibility

Status: implemented

English | [中文](2026-08-14-desktop-shell-shutdown-and-reaping-hardening.zh.md)

## Problem

Four review findings (evidence chain in `apps/desktop/DESKTOP-REVIEW.md`) all reduced to one theme: the shell trusted the happy path. Shutdown escalation watched only the direct child, so a process group that outlived its leader never received SIGKILL, and the log-reader threads joined without a deadline, so any survivor holding the pipe's write end hung tray-quit forever. Stale-sidecar reaping ran `ps -p <pid> -o command=` unconditionally, which on Windows finds no `ps` (or an incompatible MSYS one), making the reaper dead code exactly where crashes leave orphans. Boot failures after the WebView had navigated to the sidecar evaluated `window.__DSH_SHOW_ERROR__` on a page that never defines it, leaving a dead page with no feedback; fast pre-navigation failures could also race the splash page's own script load. A sidecar that died mid-session had no watcher at all.

## Decision

Escalation probes process-group liveness (`killpg(pgid, 0)`, EPERM counts as alive), mirroring `treeAlive()` in `dsh-subprocess-local`, and the direct child is reaped inside the probe so a zombie leader cannot hold a dead group answerable. Readers became cancellable and their join bounded: Unix pipes are set non-blocking at spawn and readers poll an `Arc<AtomicBool>` stop flag between `WouldBlock` retries; Windows duplicates the pipe read handles and shutdown calls `CancelIoEx` to abort pending reads; a final watchdog join leaks a stuck reader rather than hanging exit — cancellation is the mechanism, the watchdog is the last resort. Stale reaping proves identity before killing: Unix keeps the `ps` command-line match it always had; Windows pid files carry a third line with the process creation time (`GetProcessTimes`), compared against a fresh `OpenProcess` read — pid reuse changes the creation time, so a reused pid never matches. Reaping kills the whole group on Unix (TERM, 5s, KILL) and `taskkill /T /F` with explicit null stdio on Windows (inherited console handles plus `CREATE_NO_WINDOW` can fail the spawn under concurrency). Boot failures navigate back to the captured splash URL before evaluating the error hooks, retry the idempotent eval past the page-load race, and `DSH_DESKTOP_BOOT_FAIL=client-ready` makes the post-navigation path scriptable; a watcher thread polls the supervisor after a successful boot and surfaces an unexpected exit on the splash page. Alongside: a recorded workspace that no longer exists falls back to `~/Documents` instead of failing the spawn, `DSH_WORKSPACE` overrides are never persisted into `desktop-state.json`, the log rotates only after the home lock is held, and the sidecar environment whitelist gained the Windows layout/tooling variables (`APPDATA`, `COMSPEC`, `PATHEXT`, `ProgramFiles`, …) plus `TZ`, `TERM`, `SSH_AUTH_SOCK`, and CA-bundle overrides, because every tool subprocess the agent spawns inherits exactly that environment. The web bootstrap nonce TTL moved from 30s to 120s: the window spans the shell's whole boot budget, and a first launch under real-time AV scanning can spend 15s of it before the ready line.

## Alternatives considered

**Leaking reader threads instead of cancelling them.** Rejected as the mechanism: dshd is a resident tray app, and the mid-session exit watcher can fire `request_stop` while the app keeps running, so a blocked reader would persist for the process lifetime. Kept only as the watchdog's last resort.

**pid+creation-time identity on every platform (the PostgreSQL `postmaster.pid` design).** Rejected as the cross-platform commitment: macOS would need blind `proc_pidinfo` FFI and Linux `/proc` parsing to replace a `ps` match that already works there; the broken platform is Windows, so the creation-time identity is Windows-only and the record format is platform-shaped.

**Removing `panic = "abort"` so the crash-log hook runs.** Rejected after experiment: panic hooks run under the abort strategy too (verified with `rustc -C panic=abort`), so the hook was never dead code; abort also takes a panicking boot thread down with the process instead of parking the window on a splash forever.

**Raising the nonce TTL from the first consume instead.** Rejected: consume is the single use; the window that must grow is the one before it.

## Consequences

Tray quit, ctrl-c, and the exit watcher all converge on the same bounded shutdown (grace + 2s reap + 2s reader deadline). Windows reaping now kills verified orphans and refuses unverifiable records; a pid file written before this change is discarded unreaped once. `cargo test` runs green on a real Windows host (the identity test spawns, reaps, and spares live pings), and the Unix group-escalation test mirrors the TERM-trapping case from `dsh-subprocess-local`. Unix shutdown drains for the grace window; Windows never did and still does not — documented as a known limit rather than papered over.
