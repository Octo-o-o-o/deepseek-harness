# Agent Note: The signed macOS desktop release is a repository command

Status: implemented

English | [中文](2026-08-14-desktop-mac-signed-release.zh.md)

## Problem

The first notarized `dshd` DMG was produced by a sequence of shell steps that lived outside the repository. The steps were real — deep signing, DMG creation, `notarytool submit`, `stapler staple` — but nothing in the tree recorded them, so the published artifact could not be reproduced from a checkout, and `apps/desktop/README.md` still stated that the application was unsigned and that notarization was out of scope. The workflow file carried the same claim as a comment.

That script also had three defects that only a repeated release would expose. It defaulted to one person's `Developer ID Application` identity, so the command was correct on exactly one machine. It signed inner binaries inside a `find | while` subshell, where the failure branch's `exit 1` leaves the enclosing script running, so an unsignable file produced a DMG that would fail notarization minutes later. And it selected inner binaries by the executable bit, which does not describe a native addon shipped at mode 0644.

## Decision

`scripts/release/desktop-mac.ts` owns the release, beside the repository's other release sequences, and runs as `pnpm run release:desktop-mac`. It performs the repository build, sidecar pack, `tauri build`, sidecar embed, signing, DMG creation, notarization, and stapling in one command.

Signing follows the embed step. Tauri drops the sidecar's symbolic links, so `pack-sidecar.mjs embed` copies the Node runtime and the deployed CLI into `Contents/Resources` after the bundle exists; a signature taken before that copy does not cover the payload the user runs.

Membership in the signing set is decided by the Mach-O magic number in each file's header rather than by its mode, and the set is signed deepest path first so the bundle seal is taken last. The walk runs in this process, so an unsignable file ends the release where it occurs. `--deep` is absent: it re-signs everything it reaches with the outer invocation's arguments, which would replace the Node runtime's entitlements with the application's.

Entitlements are granted per executable. `entitlements.node.plist` grants JIT, unsigned executable memory, and the library-validation exemption to the embedded Node runtime alone; the application binary and the helper tools the sidecar spawns are signed under the hardened runtime with no entitlements at all. Because the grants are per file and a later pass over the bundle can replace them, the release reads the Node runtime's entitlements back and fails when they are gone — the alternative is an application that starts and then dies when V8 first compiles.

The bundle carries `LICENSE` and the generated `THIRD_PARTY_NOTICES.md` in `Contents/Resources`, and the release refuses to sign a bundle missing either: attribution obligations attach to the artifact a person received, not to the repository they never cloned.

The preflight runs before the build, not before the upload. It refuses a non-macOS host, a Keychain with no `Developer ID Application` identity, and an identity choice with more than one candidate, which `DSH_SIGN_IDENTITY` resolves — an implicit first match would sign a release with an unintended certificate. Notarization credentials are one of three complete groups: a Keychain profile, the Apple ID trio, or the App Store Connect key trio. A partially supplied group names its missing members instead of falling through to the next group, because a typo in one variable would otherwise present as "no credentials configured".

The build and pack steps receive an environment with every credential-shaped name removed, not merely the Apple ones, because the sidecar pack runs an `npm install` that executes dependency lifecycle scripts and needs no credential of any kind. A failing step reports its command name and exit status only: `notarytool` takes an app-specific password as an argument, so an error echoing its arguments would put that password in the terminal and in CI logs.

CI keeps producing unsigned artifacts on both platforms. Wiring the signed path into CI requires provisioning the Developer ID certificate and notarization credentials as repository secrets, which is a separate decision; the workflow comments now state that position instead of describing certificates as nonexistent.

## Alternatives considered

**Sign with `tauri build`'s own macOS signing configuration.** Tauri signs the bundle it creates, which is before the sidecar is embedded. The signature would not cover the Node runtime or the CLI, and the embed step would invalidate it.

**Keep the shell script and move it into the repository unchanged.** The identity default, the swallowed inner-signing failure, and the executable-bit filter are each a silent wrong-artifact path. A release command is the wrong place to keep failures that surface only at Apple's notarization service.

**Grant the Node exemptions to the whole bundle.** One entitlements file for every executable is shorter, and it hands JIT and unsigned executable memory to the application binary and to every helper the sidecar spawns, none of which compile code at run time.

**Let the preflight pick the first Developer ID identity when several exist.** It removes one variable from the common case and silently signs with an unintended certificate in the uncommon one.

## Consequences

A maintainer holding the Developer ID identity reproduces the published DMG with one command, and the README states what the repository actually does. The preflight's checks are pure functions over injected process inputs, so `scripts/release/desktop-mac.spec.ts` covers the platform refusal, absent and ambiguous identities, each complete and partial credential group, the credential withholding, and the Mach-O walk's header detection, ordering, and symbolic-link handling without invoking `codesign`.

The full command remains unproven by automation: it needs a Developer ID identity, notarization credentials, and Apple's service, none of which exist in CI or in a test. Its non-pure half is therefore covered by the same evidence any release carries — a mounted DMG passing `codesign --verify --strict`, `spctl --assess`, and `stapler validate`.
