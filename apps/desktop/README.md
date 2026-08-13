# dshd

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
│    --desktop-token <per-launch hex>         │
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

`scripts/pack-sidecar.mjs` steps: `deploy`, `runtime`, `check`, `embed` (after `tauri build`). Self-check requires a ready line within 15s, `GET /` 200 with `__DSH_BOOT__`, and SIGTERM exit 0, with `PATH=/usr/bin:/bin:/usr/sbin:/sbin`.

A packaged macOS `.app` is typically about 450MB (Node + production deploy). Windows unpack of the Node zip is implemented in the pack script; running it is CI-only on this machine.

## Data directory and logs

| | macOS | Windows |
|---|---|---|
| `DSH_HOME` | `~/Library/Application Support/DeepSeekHarness` | `%APPDATA%\DeepSeekHarness` |
| sidecar cwd | `desktop-state.json` → `workspace`, else `~/Documents` | same, else user home |
| sidecar log | `$DSH_HOME/logs/sidecar.log` (rotates at 50MB) | same |
| panic log | `$DSH_HOME/logs/crash.log` | same |
| lock | `$DSH_HOME/desktop.lock` (`flock`) | exclusive `share_mode(0)` |

First launch copies `sessions`, `settings`, `attachments`, `storages`, and `profiles` from `~/.dsh` (or `DSH_LEGACY_HOME`) when `migration-state.json` is absent. Credentials are not copied. A failure restores `migration-backup-<ts>`. `DSH_DESKTOP_MIGRATE_FAIL=1` injects that failure for tests. A second process that cannot take the lock shows “另一个 DeepSeek Harness 进程正在使用数据目录” and does not spawn.

## Token

The shell always generates a per-launch hex token and passes `--desktop-token`. The Web index injects `window.__DSH_TOKEN__`; the connection layer sends `X-DSH-Token` / cookie `dsh-token`. The token is not put in the URL or logs. `dsh web` without the flag is unchanged.

## Known limits

- Windows process-tree kill (Job Object) and `share_mode(0)` lock are compiled but **not locally verified**. `rustup target add x86_64-pc-windows-msvc` failed in this environment (rustup cache). CI owns the Windows run.
- Windows sandbox remains partial, same as the CLI.
- WebView2 presence detection / installer prompt is not wired.
- The `.app` is unsigned; notarize is out of scope.
- `open` of the `.app` from a sandbox may fail (`LSOpen` -54); launching `Contents/MacOS/dshd` still starts the sidecar.
