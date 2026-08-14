# Agent Note: The desktop build carries this fork's own distribution identity

Status: implemented

English | [中文](2026-08-14-desktop-fork-distribution-identity.zh.md)

## Problem

The desktop application shipped under `com.deepseek-ai.dsh.desktop`, named DeepSeek as its publisher, and opened a window titled `DeepSeek Harness Desktop`. This fork is a community desktop deployment of the upstream Web surface, not a DeepSeek product ([downstream deployment](../architecture/2026-08-14-desktop-shell-downstream-deployment.md)), and it is signed with this fork's Developer ID rather than DeepSeek's.

A bundle identifier is the name macOS resolves for launch services, defaults, and the keychain, so an upstream-shaped identifier would let a community build and a hypothetical official one collide in those registries. The publisher and window title made a claim about origin that the artifact cannot support.

## Decision

The bundle identifier is `com.octoooo.dshd`, the publisher is the account that signs and releases it, the short description says `Unofficial`, and the window title is `DeepSeek Harness Desktop (Unofficial)`.

The disclaimer appears once, in the window title, rather than on every surface. The title is present whenever the application is, it is the string the window manager reports, and one clear statement is read while a repeated one is skipped. The sidebar keeps naming the product it runs, because that name is accurate: this is the DeepSeek Harness Web client.

Nothing about the runtime changes. The data directory stays `$DSH_HOME`, defaulting to `~/.dsh`, which is derived from the home directory rather than from the bundle identifier, so sessions, settings, and workspaces are untouched by the rename and stay shared with the npm CLI.

## Alternatives considered

**Keep the upstream identifier.** It preserves an in-place upgrade path from earlier builds of this fork, and it registers a community binary under an identifier that names another party, which is the collision the identifier exists to prevent.

**Put the disclaimer in the product name.** `dshd (Unofficial)` would reach the Dock, the Applications folder, and every file dialog. That is a permanent cost paid in every list a person reads, for a fact they need once.

**Add an About panel to carry it.** macOS builds one from the bundle metadata, but reaching it takes a menu the application does not otherwise need, so the disclaimer would be true and unread.

## Consequences

macOS resolves an application by its identifier, so this build is a new application rather than an upgrade of one installed from an earlier release: the previous `dshd` stays until it is removed, and Launch Services, saved window state, and any per-application permission grant start empty. User data is unaffected because it never lived under the identifier.

`dsh web` from the npm CLI is untouched; the identity described here belongs to the packaged desktop application alone.
