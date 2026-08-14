# Agent Note: dshd is a downstream desktop deployment of the shipped Web surface

Status: implemented

English | [中文](2026-08-14-desktop-shell-downstream-deployment.zh.md)

## Problem

Upstream ships two application assemblies, `apps/cli` and `apps/web`, and no packaged application. Reaching the interactive product requires Node, a terminal, and `npx @deepseek-ai/dsh web`, and the README states that the project is in developer preview and will take compatibility-breaking changes.

The repository also removed its terminal frontend. `10bb9cbf4a` (2026-08-04) deleted the TUI package and the legacy `dsh` entrypoints, 28905 lines. The reason is recorded, and it is a maintenance and inventory argument rather than a judgment about users: after the implicit terminal application was removed, the package had no shipped composition, its only remaining consumer was the project generator, and keeping it "made the repository's supported application inventory misleading" ([removal decision](../simplification/2026-08-04-remove-tui-package.md)). That note's consequence is that Web is the shipped interactive surface, and it states what a new frontend must bring: a named product or deployment, an explicit package boundary, a concrete interaction provider, and assembled lifecycle and transcript acceptance.

So the gap this fork addresses is not a missing interface — the Web client already is the interface — but a missing way to install and start it without a Node toolchain and a command.

## Decision

`dshd` is a downstream deployment of the existing Web surface, not a proposal to move a desktop frontend into upstream. It supervises one `dsh web` sidecar on loopback and loads the client the sidecar already serves; the process lifetime, data directory, and token rules are owned by their own notes ([supervisor](../feature/2026-08-14-desktop-sidecar-supervisor.md), [home migration](../feature/2026-08-14-desktop-home-migration.md), [per-launch token](../feature/2026-08-14-desktop-per-launch-token.md)).

The removal decision's conditions are the ones this deployment answers. `dshd` is a named product with a signed and notarized macOS artifact ([release chain](../process/2026-08-14-desktop-mac-signed-release.md)), an explicit package boundary at `apps/desktop`, a concrete supervisor rather than a reusable frontend abstraction, and assembled acceptance: a packaged self-check requiring the sidecar's readiness line, an HTTP 200 carrying `__DSH_BOOT__`, and a clean `SIGTERM` exit under a stripped `PATH`.

Keeping it downstream is the point rather than a limitation. The cost the removal decision refused — a product-sized frontend plus, here, a platform release chain of signing, notarization, and installer formats — lands on this fork. Upstream's application inventory keeps describing exactly what upstream maintains.

The developer-preview position is preserved rather than contradicted. `dshd` adds no protocol field, client capability, or compatibility promise; it packages the same client from the same sidecar, so an upstream breaking change reaches a `dshd` user exactly as it reaches an `npx` user. This fork's position is that a preview is a reason to state the risk plainly, not a reason to keep the product behind a toolchain: an incomplete product that people can try, report on, and grow alongside is worth more than a closed door.

Tauri 2 carries the shell. The Host is already a separate Node process, so an Electron shell would ship a second browser engine for chrome that the system WebView provides, on top of the Node runtime the sidecar needs regardless. The price is explicit and paid by this fork: Rust enters a TypeScript and Node repository, CI needs a Rust toolchain, and `cargo fmt`, `cargo clippy`, and `cargo test` become gates. The first bill is already visible — the Windows process-tree kill and the exclusive directory lock compile but could not be verified on the build machine, so CI owns that signal.

## Alternatives considered

**Contribute the desktop application upstream.** The removal decision's cost argument applies unchanged, and a packaged application adds a platform release chain that a developer-preview harness has no current reason to carry. A fork can carry it, and any part upstream later wants can be offered then.

**Build the shell on Electron.** It keeps one language across the repository, and its release tooling is more mature — a parallel community fork produced a signed, notarized DMG on it. Rejected on payload: the second engine buys chrome that the system WebView already provides, while the Node sidecar remains either way. The accepted cost is a Rust toolchain.

**Reintroduce the TUI instead.** The removal decision's bar applies to a terminal frontend as much as to this one, and a terminal frontend still leaves the terminal on the user's path, which is the step this deployment exists to remove.

**Ship a browser shortcut or a PWA over the existing server.** Rejected because it still requires an installed Node and a manually started server. The install-and-start step is the whole subject.

**Wait for upstream to stabilize before packaging anything.** Rejected as the fork's product judgment: the risk is disclosable and the artifact is replaceable, while a person who cannot start the product contributes no feedback at all.

## Consequences

A person installs one artifact and double-clicks it, and because the data directory is the CLI's own `~/.dsh`, sessions and settings are shared with `npx @deepseek-ai/dsh web` rather than duplicated.

This fork owns the platform release chain, the Rust toolchain, WebView differences across platforms, and the work of tracking upstream's breaking changes. Because `dshd` adds no capability to the client or the protocol, a future upstream decision about a desktop surface is not constrained by anything decided here.

The reasoning recorded above about upstream is limited to what the repository states — a maintenance and inventory argument in the removal decision, and a developer-preview banner in the README. This note makes no claim about DeepSeek's intent beyond those records.
