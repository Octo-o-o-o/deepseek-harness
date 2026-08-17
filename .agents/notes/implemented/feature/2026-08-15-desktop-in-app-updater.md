# Agent Note: In-app updater for the desktop shell

Status: implemented

English | [中文](2026-08-15-desktop-in-app-updater.zh.md)

## Problem

The desktop application had no update channel. Everyone who downloaded a build stayed on it until they happened to notice a new release and re-downloaded it by hand. That cost grows with every user and every release, and it blocks the delivery of security fixes specifically: `dshd-v0.1.7` shipped a local privilege-escalation fix that its own users had no way to learn about.

The absence also inverts the value of later work. Any improvement — notifications, the API guard, a crash fix — reaches nobody automatically until an update channel exists, so the channel has to come first even when other items look more interesting.

## Decision

`tauri-plugin-updater` with a minisign key pair. The public key ships inside `tauri.conf.json`; the plugin verifies each artifact's detached signature before writing anything, so a compromised endpoint can withhold or replay a release but cannot introduce code. The private key stays offline and never enters the repository, R2, or a log.

**The update entry lives in the tray, not the Web UI.** `frontendDist` points at the GUI shared with the browser surface, where an in-app updater is meaningless; putting the control there would require a desktop-only branch in shared code. The tray item re-labels itself from the check result (`Check for Updates` / `No Updates Available` / `Update to <version>` / `Installing Update…`) and is clickable only in the states where clicking does something.

**The sidecar is stopped only after verified bytes are in hand.** The installer replaces the bundled Node runtime and the deployed CLI; on Windows an open file cannot be replaced at all. `run_install` downloads and signature-checks first, then `request_stop()`, then `install`. A failed download leaves the running session intact. A failed `install` after that stop cannot keep the session: the shell returns the main window to the start page so Restart is reachable.

**Check failures are silent.** An offline machine or an endpoint outage must not interrupt a session, so a failed check logs and leaves the tray showing the last known status. A `busy` flag makes a second click a no-op instead of a concurrent install of the same artifact.

`latest.json` is produced by `scripts/release/updater-manifest.ts`. Tauri signs the *artifact*, not the manifest — the manifest only carries the signature beside the URL — so publish order is itself a safety property: the script fails when `<artifact>.sig` is missing, which means a manifest can only be written after the artifacts it points at exist and are signed.

## Release-chain facts this cost five failed builds to establish

Each of these is unavoidable on a first signed release, and none is discoverable from the existing docs:

1. **Tauri does not accept `TAURI_SIGNING_PRIVATE_KEY_PATH` here.** The build reports `A public key has been found, but no private key`. Pass the key *content* via `TAURI_SIGNING_PRIVATE_KEY`.
2. **The signing key cannot reach the build through the normal environment.** `buildEnvironment` in `scripts/release/desktop-mac.ts` strips every credential-shaped name (`/KEY|SECRET|TOKEN|PASSWORD/i`), and both signing variables match. That stripping is correct — `pnpm build` and the sidecar pack's `npm install` touch dependency code — so the fix is `bundleEnvironment`, which restores exactly those two names for the bundle step alone. `bundleApp()` runs only `tauri build` and installs nothing, so the exposure stops there.
3. **`notarytool store-credentials` profiles disappear.** The `dsh-notary` profile vanished twice between successful builds, and `security` never found it under the documented service name. Credentials stored with `security add-generic-password` have been stable, so the release now uses the `APPLE_ID` trio with the app-specific password read from a self-managed Keychain item.
4. **The payload manifest drifts between builds.** Upstream publishes patch releases continuously; a build hours after the last one can fail on a single package. This is the gate working, not a defect — see the [per-platform manifest note](../bug-fix/2026-08-15-payload-manifest-per-platform.md).
5. **Notarization is the last step and the most expensive to redo.** A credential problem there wastes the whole build. Verify credentials with a cheap call (`xcrun notarytool history`) before starting a release.

## Alternatives considered

**Renderer-driven update UI.** Rejected: it puts desktop-only behavior in the shared front end for no gain the tray does not already provide.

**Silent download, then install on click.** The plugin returns the downloaded bytes in memory, and the payload is ~180 MB. Holding that for an unknown period to save a few seconds at click time is a bad trade; the current flow downloads and installs on the click.

**Widening `buildEnvironment` to carry the signing key.** Rejected: it hands the key to `npm install` and to every dependency lifecycle script, which is exactly what the stripping exists to prevent.

## Verification

`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test` (72 tests) pass. `scripts/release/desktop-mac.spec.ts` covers both sides of the environment split: the bundle step restores exactly the two signing names, and the build/pack step still receives neither.

A real signed build was produced end to end and verified without publishing: notarization `Accepted`, `stapler validate` worked, Gatekeeper reported `source=Notarized Developer ID`, the bundle declares `0.1.8` / `com.octoooo.dshd`, and the binary contains both the updater endpoint and the notification command. The updater artifacts exist — `dshd.app.tar.gz` (76,261,400 bytes) with a 400-byte `.sig` — and `updater-manifest.ts` consumed that real signature.

Not verified: an actual upgrade. No published `latest.json` exists yet, so nothing has exercised check → download → install → restart against a live endpoint. That is the first thing the next release must prove.

## Consequences

The first version carrying the updater still has to be installed by hand — an update channel cannot deliver itself. Every release after it reaches existing users automatically.

Key rotation has no in-band path yet: the Tauri config holds a single `pubkey` with no keyring, so replacing the key requires a bridge release signed by the old key that embeds the new one. Losing the private key before that bridge exists leaves no safe recovery except manual reinstall. This is deferred work, not a solved problem.

The endpoint (`/updates/latest.json`) must never be cached; `site/_headers` sets `no-store` for it. The artifacts themselves use content-addressed immutable keys and can be cached indefinitely.
