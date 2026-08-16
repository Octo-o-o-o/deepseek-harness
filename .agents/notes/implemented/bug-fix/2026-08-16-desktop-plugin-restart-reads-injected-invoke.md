# Agent Note: Read Tauri's injected invoke from the sidecar page

Status: implemented

English | [中文](2026-08-16-desktop-plugin-restart-reads-injected-invoke.zh.md)

## Problem

Installing a plugin while the desktop window is open left the restart prompt invisible even when the shell would have answered `true`. The page only read `window.__TAURI__.core.invoke`. That object is the optional withGlobalTauri convenience bundle. The command function Tauri injects into every WebView — and the function `@tauri-apps/api/core` wraps — is `window.__TAURI_INTERNALS__.invoke`, delivered by the much smaller `core.js` user script. When the convenience bundle is absent, every poll reports "nothing pending" and a thrown invoke is swallowed, so the person sees no button and no error.

The product rule is not at fault: the entry is supposed to appear only while the profile manifest differs from what this sidecar composed ([feature](../feature/2026-08-16-desktop-plugin-restart-prompt.md)). The page never asked the function that was actually injected.

## Decision

`packages/client/ui-plugin-restart/src/client/shell.ts` resolves invoke from `__TAURI__.core` first (the documented withGlobalTauri path) and from `__TAURI_INTERNALS__` when that function is missing. Absence of both still means the page is not hosted by the shell. A failing command still answers `false`, so a broken IPC path cannot pin a restart prompt the person cannot act on.

The owning product rule is unchanged: the entry appears only while the profile is stale relative to this sidecar launch, confirmation still restarts the application, and `dsh web` in a browser still has neither global.

## Alternatives considered

**Show the restart entry unconditionally.** Violates the owning feature's visibility rule: the control exists to say the composition is stale, not to offer a general restart.

**Ask the sidecar over HTTP whether plugins are pending.** Rejected in the owning note: both halves of the fact belong to the shell, and restart still needs IPC — a button that appears via HTTP and then fails to restart is worse than a missing button.

**Depend on `@tauri-apps/api`.** That package's `invoke` is a thin wrapper around `__TAURI_INTERNALS__.invoke`. Pulling it into a harness client plugin that also ships in the CLI payload buys no extra contract and adds a desktop-only runtime to a package that must stay inert on `dsh web`.

**Grant the two commands from a static capability with `http://127.0.0.1:*`.** Rejected in the owning note: it would hand restart to any local process that can bind a loopback port.

**Surface a broken-state banner when invoke throws.** An unreachable shell must not pin a control the person cannot use; the existing "answer false" rule stays.

## Verification

`pnpm vitest run packages/client/ui-plugin-restart` — 19 tests across two files. The shell suite covers a missing host, a convenience-only host, an internals-only host, a host that carries both (convenience wins), a thrown command answering "nothing pending", and a restart forwarded through either path.

Not yet covered: a packaged-app run proving the WebView actually exposes `__TAURI_INTERNALS__.invoke` while omitting `__TAURI__.core`. The next signed build must confirm the prompt appears after `dsh plugin add` with the window already open.

## Consequences

A plugin install that rewrites the profile manifest while the window is open can show the sidebar entry as soon as either injected invoke path answers `true`. A browser tab on `dsh web` still has neither global, so the entry stays absent there. The shell's stamp, runtime capability, and two-command grant are unchanged.
