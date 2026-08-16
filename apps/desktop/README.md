# dshd

English | [中文](README.zh.md)

Tauri 2 shell that starts a local `dsh web` sidecar on `127.0.0.1`, waits for the ready line, checks `__DSH_BOOT__` and `host.describe`, then loads the existing Web GUI in a WebView.

```
┌─────────────────────────────────────────────┐
│  Tauri 2 shell (tray, single-instance)      │
│    spawn node → parse ready line            │
│    GET /  +  POST /api/host.describe        │
│    navigate WebView to http://127.0.0.1:N   │
└──────────────────┬──────────────────────────┘
                   │ loopback only
                   ▼
┌─────────────────────────────────────────────┐
│  Node sidecar (bundled runtime + deploy)    │
│    dsh web --port 0 --host 127.0.0.1        │
│    env DSH_DESKTOP_TOKEN + BOOTSTRAP_NONCE  │
└─────────────────────────────────────────────┘
```

## Development

From this directory, with a built CLI (`pnpm run build` at the repo root):

```sh
cd src-tauri
cargo test
cargo run
```

`cargo run` uses `node` on PATH and `apps/cli/lib/bin.js` from the checkout. Override with `DSH_NODE_PATH` / `DSH_WEB_BIN`. `DSH_HOME` and `DSH_WORKSPACE` override the data directory and sidecar cwd.

Gates (cwd = `src-tauri`):

```sh
cargo fmt --check
cargo clippy -- -D warnings
cargo test
```

## Packaging

```sh
# repo root: production deploy + pinned Node v24.19.0 + PATH-stripped boot
pnpm --filter @deepseek-ai/dshd run pack

# this package: unsigned .app, then re-copy the sidecar (Tauri drops symlinks)
pnpm --filter @deepseek-ai/dshd run build
```

### Signed and notarized macOS DMG

```sh
# Store both secrets once. `security` items have proven durable here; the
# `notarytool store-credentials` profile has silently disappeared between
# successful builds more than once, so the release reads the app-specific
# password from a Keychain item of its own instead.
security add-generic-password -a "$USER" -s dshd-notary-pw -w      # app-specific password
security add-generic-password -a "$USER" -s dshd-updater-key -w    # updater private-key password

TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.dshd-updater.key)" \
TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$(security find-generic-password -a "$USER" -s dshd-updater-key -w)" \
APPLE_ID="<Apple ID>" APPLE_TEAM_ID="<Team ID>" \
APPLE_APP_SPECIFIC_PASSWORD="$(security find-generic-password -a "$USER" -s dshd-notary-pw -w)" \
pnpm run release:desktop-mac
```

Pass the updater key as **content**, not as `TAURI_SIGNING_PRIVATE_KEY_PATH`: the build otherwise reports `A public key has been found, but no private key`. The signing variables reach only the bundle step, through `bundleEnvironment` — `buildEnvironment` strips every credential-shaped name because `pnpm build` and the sidecar pack's `npm install` touch dependency code ([updater note](../../.agents/notes/implemented/feature/2026-08-15-desktop-in-app-updater.md)).

Verify notarization credentials before starting, since notarization is the last and most expensive step to redo:

```sh
xcrun notarytool history --apple-id "<Apple ID>" --team-id "<Team ID>" \
  --password "$(security find-generic-password -a "$USER" -s dshd-notary-pw -w)"
```

`scripts/release/desktop-mac.ts` runs the whole release: repository build, sidecar pack, `tauri build`, sidecar embed, signing, DMG, notarization, and stapling. Signing follows the embed step because a signature taken before it does not cover the Node runtime or the deployed CLI. It covers every Mach-O file in the finished bundle, selected by file header rather than by the executable bit so native addons shipped without `+x` are included, and takes the innermost path first so the bundle seal is last. `src-tauri/entitlements.node.plist` grants JIT, unsigned executable memory, and the library-validation exemption to the embedded Node runtime alone; the application binary and the sidecar's helper tools are signed under the hardened runtime with no entitlements. The bundle also carries `LICENSE` and the generated `THIRD_PARTY_NOTICES.md`, and the release refuses to sign without them.

The preflight runs before the build, so a credential problem costs seconds rather than a full pack. It refuses a non-macOS host, a Keychain holding no `Developer ID Application` identity, an identity choice it would otherwise have to guess (set `DSH_SIGN_IDENTITY`), and missing or partial notarization credentials. Credentials come from `APPLE_KEYCHAIN_PROFILE`, the `APPLE_ID` / `APPLE_APP_SPECIFIC_PASSWORD` / `APPLE_TEAM_ID` group, or the `APPLE_API_KEY` / `APPLE_API_KEY_ID` / `APPLE_API_ISSUER` group; a partial group is an error rather than a fallback. Release variables are withheld from the build and pack subprocesses and reach only `codesign` and `notarytool`.

Verify a finished DMG. The notarization ticket is stapled to the DMG, so `stapler validate` takes the disk image; the mounted application is checked by Gatekeeper, whose `source=Notarized Developer ID` is what a double-click resolves:

```sh
xcrun stapler validate apps/desktop/dist/dshd-0.1.0-arm64.dmg
codesign --verify --deep --strict --verbose=2 "/Volumes/dshd/dshd.app"
spctl --assess --type execute --verbose=4 "/Volumes/dshd/dshd.app"
```

`scripts/pack-sidecar.mjs` steps: `deploy`, `runtime`, `check`, `embed` (after `tauri build`). Self-check requires a ready line within 15s, `GET /` 200 with `__DSH_BOOT__`, and SIGTERM exit 0, with `PATH=/usr/bin:/bin:/usr/sbin:/sbin`.

These probes use `fetch` and `curl`, which decode `Transfer-Encoding: chunked` — the framing the sidecar always sends — while the shell's own health client decodes it in `http.rs`. `cargo test`, not the pack self-check, is what holds that client to real framing.

A packaged macOS `.app` is typically about 320MB (Node + production deploy). Windows unpack of the Node zip is implemented in the pack script; running it is CI-only on this machine.

## Data directory and logs

| | macOS | Windows |
|---|---|---|
| `DSH_HOME` | `~/.dsh` | `~/.dsh` |
| sidecar cwd | `desktop-state.json` → `workspace`, else `~/Documents` | same, else user home |
| sidecar log | `$DSH_HOME/logs/sidecar.log` (rotates at 50MB) | same |
| panic log | `$DSH_HOME/logs/crash.log` | same |
| lock | `$DSH_HOME/desktop.lock` (`flock`) | same file, `LockFileEx` byte-range lock |

The data directory is the one the npm CLI uses, so sessions, settings, and workspaces are shared live with `npx @deepseek-ai/dsh web`; both may run at once, each on its own OS-assigned port. `desktop.lock` is taken by this shell only, so it excludes a second `dshd`, not a CLI server.

First launch copies `sessions`, `settings`, `attachments`, `storages`, and `profiles` from the pre-unification desktop home — `~/Library/Application Support/DeepSeekHarness`, `%APPDATA%\DeepSeekHarness`, or `DSH_LEGACY_HOME` — when `migration-state.json` is absent and `~/.dsh` holds none of those directories yet, so existing CLI data is never overwritten. Credentials are not copied. A failure restores `migration-backup-<ts>`. `DSH_DESKTOP_MIGRATE_FAIL=1` injects that failure for tests, and `DSH_DESKTOP_BOOT_FAIL=client-ready` injects a post-navigation boot failure the same way. A second process that cannot take the lock shows “another dshd instance is using the data directory” and does not spawn.

`sidecar.pid` records the sidecar's process id, entry script, and (Windows) process creation time. The next boot reaps that process only when the recorded identity still matches — entry script via `ps` on Unix, creation time on Windows — so a pid reused by a CLI `dsh web` or by an unrelated process is left alone; an unverifiable record is discarded without a kill. Shutdown escalates on process-group liveness on Unix (TERM, then KILL to the whole group) and terminates the Job Object on Windows; after a successful boot the shell watches the sidecar and surfaces an unexpected exit on the splash page.

## Environment

The sidecar receives the names listed in `src-tauri/src/env.rs` and nothing else, so a credential exported in a shell profile for an unrelated service never reaches the agent. Values come from the application's own environment, with `PATH` as the single exception: it comes from the user's login shell, because an application opened from the Dock inherits `/usr/bin:/bin:/usr/sbin:/sbin` from the launch daemon and the agent's `bash` tool would then find none of the tools the user installed.

The probe runs `$SHELL -ilc` once per launch with `DSH_RESOLVING_ENVIRONMENT=1` set, so a profile can skip work meant for an interactive session, and reads the `env -0` block between its own markers. A shell that fails or takes longer than 5s leaves the launch environment in place. `DSH_DESKTOP_SHELL_ENV=0` skips the probe.

## Links and dropped files

The WebView may load the bundled start page and the loopback sidecar. Any other navigation is refused and opened in the default browser instead: the window has no address bar, so page content must not be able to replace the application UI, and `target="_blank"` links would otherwise do nothing at all.

`dragDropEnabled` is off so the Web UI receives HTML5 drop events itself. Tauri's own drag-drop handler consumes the event before the page sees it, which would leave a file dropped on the conversation with no effect.

## Token

The shell always generates a per-launch hex token and a bootstrap nonce and injects them as `DSH_DESKTOP_TOKEN` / `DSH_DESKTOP_BOOTSTRAP_NONCE`. The nonce reaches the page through the navigation URL's fragment (`#dshd-nonce=…`), which user agents never put on the wire, so it appears in no response body; the page strips it from session history once read. Serving it inside the index instead would hand it to any local process able to reach the loopback port, because loopback carries no user identity. `POST /__dshd_bootstrap` then sets an origin-scoped HttpOnly `dsh-token` cookie. Two places read it: the webserver's admission guard, ahead of route matching, so the token gates the whole `/api` namespace including routes a profile-patch plugin registers; and connection's own check on each dedicated RPC channel, which plugins mount at top-level paths of their own. The cookie is scoped to the origin rather than to `/api` because a browser cannot attach `X-DSH-Token` to the client RPC's fetches, so a narrower scope would leave every plugin channel answering 401 for the whole launch. The shell polls `/__dshd_status` with `X-DSH-Bootstrap` and calls `/api/host.describe` with `X-DSH-Token`, after the WebView client posts `/__dshd_ready`. The token is not put in argv, any URL, or logs. `dsh web` without those env vars is unchanged.

## Opening on another screen

The sidecar stays on `127.0.0.1`. A Desktop-only share gateway in the same process issues a revocable `dsh-share` cookie to other browsers and injects the launch token only on the hop to loopback. Menu **Open in Browser** (⌘⇧B; tray: **在浏览器中打开**) opens `http://127.0.0.1:<gateway>/` during a short loopback pairing window; the URL has no ticket. **Use on Another Device…** (**在其他设备上使用…**) opens a shell window, not a Web sidebar.

Nearby is off by default. Turning it on binds the chosen LAN IPv4 (not `0.0.0.0`); the QR is a `/p/<ticket>` interstitial that a same-origin POST consumes. LAN is plaintext HTTP. **Anywhere** runs a foreground `tailscale serve` at the gateway loopback port on an HTTPS port this feature owns, and never `off`s an existing 443 Serve. Quit stops that child. Remote browsers cannot pick folders, change settings, or change credentials: those methods stay 403 because the gateway rewrites their Host to `dshd.share.internal`. Switches persist in `desktop-state.json` as `{ nearby, tailscale }` after the listen or Serve actually reaches that state; a corrupt file is left untouched.

`overlay.rs` still refuses `--host 0.0.0.0` and `--trusted-host` on the sidecar argv.

## Known limits

- Apple Silicon only. The bundle and its Node runtime are `arm64`, and `minimumSystemVersion` is the runtime's own floor, macOS 13.5; an Intel Mac runs `npx @deepseek-ai/dsh` instead. Building an `x86_64` payload additionally requires `pruneNonHostArtifacts` to stop deleting `node-pty/prebuilds/darwin-x64` and a terminal check on a real x64 host.
- Windows shutdown terminates the Job Object immediately — no drain window; Unix drains for 5s before the forced group kill. Sessions are transactionally logged, so either path is crash-safe.
- Windows sandbox remains partial, same as the CLI.
- A plugin's exact `/api` route wins over the connection prefix, so one named after an RPC method (`/api/session.create`) replaces that method for the whole launch. Authentication is unaffected — the admission guard covers every `/api` path — and the composition is reported on stderr at startup, but nothing prevents the collision.
- WebView2 presence detection / installer prompt is not wired.
- Only macOS has a signed release path. The `build` script and both CI platform artifacts stay unsigned, and Windows and Linux installer formats and their signing remain release work.
- `open` of the `.app` from a sandbox may fail (`LSOpen` -54); launching `Contents/MacOS/dshd` still starts the sidecar.
