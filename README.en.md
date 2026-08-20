# DeepSeek Harness Desktop (dshd)

[中文](README.md) | English

[DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness) in a desktop app you can double-click. **Unofficial community project, not affiliated with DeepSeek.**

[![Download macOS](https://img.shields.io/badge/Download-macOS%20Apple%20Silicon-000?logo=apple)](https://github.com/Octo-o-o-o/deepseek-harness-desktop/releases/latest) [![Download Windows](https://img.shields.io/badge/Download-Windows%20x64-0078D4?logo=windows)](https://github.com/Octo-o-o-o/deepseek-harness-desktop/releases/latest) [![Website](https://img.shields.io/badge/Website-dshd.octoooo.com-f0ede4)](https://dshd.octoooo.com)

## Credit where it belongs

The core is not mine. `dsh` is the agent harness [DeepSeek AI](https://deepseek.com) released as open source, built on an architecture where **everything is a plugin** and powered by [Cordis](https://github.com/cordiverse/cordis), whose design is described in [_A Programming Paradigm for Spatiotemporal Composability_](https://github.com/cordiverse/paper).

Everything hard — the agent loop, the plugin system, event-sourced sessions, the tool and permission model, the Web GUI — comes from the official project under MIT. This repository **changes none of that core logic**. It does one thing: wraps it in a desktop shell that needs no terminal and no Node installation, and gets signing, notarization, and distribution working end to end.

Having an open-source harness this well structured — down to documented event contracts — is the precondition for this project existing at all. Thanks to DeepSeek for releasing it.

## Download

| Platform | File | Notes |
|---|---|---|
| macOS · Apple Silicon | `dshd-0.1.0-arm64.dmg` | Developer ID signed, Apple notarized and stapled |
| Windows · x64 | `dshd_0.1.0_x64-setup.exe` | NSIS installer, recommended |
| Windows · x64 | `dshd_0.1.0_x64_en-US.msi` | MSI, for managed deployment |

Get them from the [latest Release](https://github.com/Octo-o-o-o/deepseek-harness-desktop/releases/latest) or the [website](https://dshd.octoooo.com). Every artifact ships a SHA-256; verify before installing.

- **macOS 13.5+, Apple Silicon only.** On an Intel Mac use `npx @deepseek-ai/dsh`.
- **Windows 10 / 11 x64.** There is no code-signing certificate, so SmartScreen blocks the first run: choose "More info → Run anyway".

## What the shell actually does

Not a web page in a window frame. Each of the following is implemented, and covered by tests or verified on real hardware:

**Works out of the box**
- Ships a pinned Node runtime (v24.19.0), so **no Node installation is required** and your existing Node is left alone.
- Launch spawns a local sidecar bound to a random `127.0.0.1` port and navigates the WebView only once it is ready. It lives in the tray; closing the window does not quit it.
- Clicking the Dock/taskbar icon restores the window, and a single-instance lock prevents a second copy.

**Data is shared with the CLI, live**
- The data home is the CLI's own `~/.dsh`, so sessions, settings, and workspaces are **shared in both directions in real time** — a session you start in `npx @deepseek-ai/dsh` shows up here immediately, and vice versa.
- An earlier desktop build (app-data home) is migrated to `~/.dsh` on first launch, and only when `~/.dsh` holds no data of its own, so existing CLI data is **never overwritten**. A failed migration rolls back.

**Process lifecycle leaves nothing behind**
- The sidecar's **whole process tree** is owned by a Job Object on Windows and a process group on Unix: force-quitting the app leaves no orphan node process.
- Liveness is judged per process group rather than per group leader (a dead leader does not mean a dead tree); KILL follows TERM only when survivors remain.
- The pid file records process creation time, so a reused pid never matches — the app cannot kill a `dsh web` you started yourself.

**The security trade-offs were taken seriously**
- Each launch mints its own token and `/api` accepts only an HttpOnly cookie. The **one-time bootstrap nonce travels in the URL fragment** — user agents never put a fragment on the wire, so a process scanning local ports cannot read it out of a response body (loopback carries no user identity, which is what makes this matter).
- The WebView has a navigation fence: only the bundled start page and the local sidecar are allowed. Links in model answers open in the system browser instead of replacing the app UI.
- The sidecar receives an allow-listed environment, so a key exported in your shell profile for another service never reaches the agent. `PATH` is the deliberate exception, taken from the login shell — otherwise an app launched from the Dock would leave the agent's bash tool unable to find your toolchain.
- Session logs are single-writer protected: writing the same session from both the desktop app and the CLI is refused with an explicit error instead of silently corrupting the log.

**The release chain is reproducible**
- The payload manifest records the resolved versions of 322/321 external dependencies in per-platform sections; any drift fails the build instead of quietly shipping a different payload.
- macOS signing covers every Mach-O file in the bundle (selected by file header, not the executable bit), and after notarization and stapling the DMG is mounted again to re-verify Gatekeeper.

## What it deliberately does not do

- **No core changes.** The plugin system, Web UI, and agent loop are the official code, untouched. For the CLI or core development, go to the [official repository](https://github.com/deepseek-ai/deepseek-harness).
- **Nothing in the cloud.** No accounts, no telemetry, no upload. App data and the local service run on your machine.
- **No key custody.** API keys use the harness's own credential layer (environment, `~/.dsh/.credentials.yaml`, `.env`); the shell collects nothing extra.
- **No Intel Mac support** for now: the bundled Node runtime is arm64.
- **No Windows code-signing certificate**, hence the SmartScreen prompt. That is not something a code change can fix.

## Maturity

The core is at the official **developer preview (rc)** stage, and the official README states there will be compatibility-breaking changes. The shell itself is still `0.1.0`. Great for trying out — **do not keep important data in it**.

## Relation to the official project

Built on [deepseek-ai/deepseek-harness](https://github.com/deepseek-ai/deepseek-harness): the core capabilities, plugin system, and Web UI all come from the official project. This repository only provides the Tauri 2 desktop wrapper, local service lifecycle management, tray and window integration, and installer builds with signed distribution.

This is a complete harness checkout, so the usual entry points work too.

### Run from npm

```sh
npx @deepseek-ai/dsh web
```

Starts the Web UI at `http://127.0.0.1:3080` by default, and a local launch also opens the default browser. An SSH launch only prints the URL. Pass `--no-open` to skip opening a browser. See the [Web UI guide](docs/user/guide/index.md).

### Run from source

```sh
git clone https://github.com/Octo-o-o-o/deepseek-harness-desktop.git
cd deepseek-harness-desktop
pnpm install
pnpm run build
pnpm dsh web
```

Desktop build and design live in [`apps/desktop`](apps/desktop/README.md); Windows packaging is in [WINDOWS-BUILD.md](apps/desktop/WINDOWS-BUILD.md).

## Development

Start with the [development guide](docs/development.md) and [architecture documentation](docs/architecture.md). For agents, follow [AGENTS.md](AGENTS.md).

## License

[MIT](LICENSE). The original DeepSeek Harness repository code and documentation are copyright DeepSeek AI.

Third-party dependencies and their licenses are disclosed in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
