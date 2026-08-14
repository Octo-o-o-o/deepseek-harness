# Agent Note: Desktop Rust licenses are disclosed from a committed inventory

Status: implemented

English | [中文](2026-08-14-desktop-rust-license-inventory.zh.md)

## Problem

`THIRD_PARTY_NOTICES.md` disclosed every npm, vendored, and Python dependency, and none of the 483 crates the desktop application links. The repository ships those crates compiled into a signed macOS artifact, so their attribution and source-availability terms apply to a distribution that disclosed nothing about them.

Only `cargo metadata` carries a crate's SPDX expression, and it needs a Cargo toolchain and a populated registry. The notices generator runs in the test lane and on machines that have neither, so it cannot call Cargo.

## Decision

`apps/desktop/src-tauri/licenses.json` is a committed inventory of name, version, SPDX expression, and repository for every crate. `pnpm run gen-desktop-rust-licenses` refreshes it from `cargo metadata`; `gen-third-party-notices.ts` reads it and renders the `Desktop Rust dependencies` section.

The inventory covers every `Cargo.lock` entry rather than one target's subset: the lock is the exact set the build resolves from, over-disclosure carries no obligation, and omitting a platform's crates would under-disclose the artifact built for it. Every generation compares the inventory's crate set against the lock and fails naming the difference, so a dependency change cannot pass unnoticed on a machine without Cargo.

The crates run through the same permissive-license policy the npm tiers use, so an unreviewed copyleft crate fails generation instead of being absorbed into a table. Two identifiers were missing from that policy rather than excluded by it: `Unicode-3.0` and `Zlib` are permissive, and their absence only meant the set had never seen them. Exceptions stay non-permissive by default, with `LLVM-exception` admitted because it only widens the grant.

The five MPL-2.0 crates are authorized by name. MPL-2.0 obliges the distributor to tell recipients how to obtain the covered source, so the rendered section names the exact versions and states that crates.io serves them permanently and that the covered files are unmodified here. The bundle carries the generated notices in `Contents/Resources`, because the obligation attaches to the artifact rather than to the repository.

## Alternatives considered

**Call `cargo metadata` from the notices generator.** It removes the committed inventory and the staleness check, and it makes a documentation gate require a Rust toolchain on every machine and in CI.

**Disclose only the crates that link into one target.** It is the smaller and more precise list, and it needs a resolve per shipped target while making a Windows artifact's disclosure depend on a macOS run.

**Reclassify MPL-2.0 as permissive.** It would silence the check for every future dependency rather than for the five crates reviewed here, and MPL-2.0's source obligation is real regardless of how the policy names it.

## Consequences

A dependency change fails the notices generation until the inventory is refreshed, which requires Cargo on the machine that refreshes it but on no other. `scripts/gen-third-party-notices.spec.ts` covers the lock comparison in both directions, a version bump, the policy classification including the authorized and admitted cases, and the rendered row for an undeclared license and repository.

Whether the shipped binary actually links a given crate is not asserted; the lock is disclosed as resolved.
