# Agent Note: Record the payload manifest per platform

Status: implemented

English | [中文](2026-08-15-payload-manifest-per-platform.zh.md)

## Problem

`payload-manifest.json` held one flat `{cli, node, packages}` table, and the `manifest` step overwrote the whole file with the resolution of whichever host ran it. The check compared in both directions: a package present in the payload but absent from the record was `+ name@version`, and one recorded but not installed was `- name`.

Optional dependencies resolve per host. A macOS payload carries `@img/sharp-darwin-arm64`; a Windows one carries `@img/sharp-win32-x64`. One flat table can therefore only ever be true for the host that wrote it.

This deadlocked the two platforms. Building on Windows reported nine differences — four `win32-x64` additions and five `darwin-arm64` removals, all same-version variant swaps with no version arrows at all. Accepting them would have flipped the file to win32-only, at which point the next macOS build would have failed symmetrically. Neither platform could record its own resolution without breaking the other, so neither could produce a release.

## Decision

Each platform owns a complete section under `platforms['<platform>-<arch>']`. The check compares only the running host's section, still in both directions, so drift detection keeps its full strength. The `manifest` step merges rather than replaces: it rewrites its own section and leaves every other one byte-identical.

A missing section for the running platform is its own diagnostic — "no section for win32-x64 (recorded: darwin-arm64); run the manifest step on this platform" — rather than a wall of additions and removals that reads like drift and invites the wrong fix.

Splitting per platform, rather than trying to separate "shared" from "platform-specific" packages, avoids a classification the data cannot support: with one platform recorded there is no way to tell which entries are shared. Duplicating the shared majority across sections costs file size and nothing else.

## Alternatives considered

**Ignore packages whose names look platform-specific.** Name-pattern matching is guesswork — `@img/sharp-darwin-arm64` is recognizable, an arbitrary optional dependency is not — and a wrong guess silently drops a real package from the comparison.

**Record the union and check only one direction** (payload ⊆ manifest). This lets a Windows build pass against a manifest containing darwin entries, but it also stops detecting removals, which is half the gate.

## Verification

macOS: `pack-sidecar.mjs deploy` reports `payload matches payload-manifest.json for darwin-arm64 (322 external package(s))`.

Windows (owner's machine): the `manifest` step recorded `win32-x64` as **323 insertions, 0 deletions** — the darwin section untouched — and the following full run reported `payload matches payload-manifest.json for win32-x64 (321 external package(s))`.

Cross-check after both sections existed: the darwin section is byte-identical to what it was before the Windows run, and of the 317 packages present in both sections, **zero disagree on version** — platform variance is confined to the platform-suffixed packages, with no version drift hiding inside it.

A later macOS build hit a genuine single-package drift (`jose 6.2.8 -> 6.2.9`); accepting it changed exactly one line and left the `win32-x64` section untouched, which is the property this note exists to guarantee.

## Consequences

The file is roughly twice as large, since the shared majority appears in both sections. That is the price of a record that is true on more than one host.

Each platform must run the `manifest` step once to register itself. That step is now safe to run from either host — it can no longer destroy another platform's record — so the earlier instruction to stop and report before accepting drift applies only to judging *whether* the drift is legitimate, not to protecting the file.
