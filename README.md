# DeepSeek Harness

English | [中文](README.zh.md)

DeepSeek Harness (`dsh`) is an open-source agent harness developed by [DeepSeek AI](https://deepseek.com).

It uses an architecture where **everything is a plugin**, and is powered by [Cordis](https://github.com/cordiverse/cordis), whose design is described in [_A Programming Paradigm for Spatiotemporal Composability_](https://github.com/cordiverse/paper).

## Developer preview

DeepSeek Harness is currently in _developer preview_ and is iterating rapidly. **THERE WILL BE COMPATIBILITY-BREAKING CHANGES.**

## Run

### Run from `npm`

Install `Node.js`, then run:

```sh
npx @deepseek-ai/dsh web
```

The command starts the Web UI, served at `http://127.0.0.1:3080` by default. See [Web UI guide](docs/user/guide/index.md).

### Desktop app (dshd)

A native desktop app for macOS (Apple Silicon) is available for users who prefer a double-click install over a terminal. **dshd** bundles the full dsh Web UI, a pinned Node runtime, and a local loopback server into a single self-contained app — no Node.js installation required.

- Download: [latest GitHub Release](https://github.com/Octo-o-o-o/deepseek-harness/releases/latest) (`dshd-*.dmg`, notarized with Apple).
- Install: open the DMG and drag `dshd` into Applications.
- The app is signed and notarized (Developer ID), so it opens with a normal double click; verify integrity with the published SHA256.
- Windows builds and the full desktop design live in [`apps/desktop`](apps/desktop/README.md).

The app uses `~/.dsh` as its data home — the same home as the npm CLI — so existing sessions, settings, and workspaces appear immediately and stay shared in both directions.

- Running dshd while an `npx @deepseek-ai/dsh web` process is active: both listen on separate loopback ports and both remain functional, but they would write the same session store concurrently. Avoid running both at once; dshd holds a directory lock against other dshd instances, while the CLI does not lock.
- Users of the earlier dshd build (app-data home) are migrated to `~/.dsh` on first launch, only when `~/.dsh` has no sessions of its own.

### Run from source

To run from a repository checkout:

```sh
git clone https://github.com/deepseek-ai/deepseek-harness.git
cd deepseek-harness
pnpm install
pnpm run build
pnpm dsh web
```

## Community and support

- Feel free to submit feedback or bug reports through [GitHub Discussions](https://github.com/deepseek-ai/deepseek-harness/discussions).
- Add the [`dsh-plugin`](https://github.com/topics/dsh-plugin) topic to your plugin repository for discoverability.
- Join <a href="https://discord.gg/Ycq5dCaS4">DeepSeek Harness Discord community</a>.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## Development

Start with the [development guide](docs/development.md) and [architecture documentation](docs/architecture.md).

For agents, follow [AGENTS.md](AGENTS.md).

## License

[MIT](LICENSE)

Third-party dependencies and their licenses are disclosed in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
