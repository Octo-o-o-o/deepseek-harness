# @deepseek-ai/dsh-client-ui-plugin-restart

English | [中文](README.zh.md)

Desktop-only sidebar action: a restart entry that appears once the profile's plugin list has moved away from what the running composition read. `dsh plugin add` rewrites the profile manifest's `dsh.profile.bundles`, but the composition took that list once at boot — `composeLive` re-reads only the patch files — so a newly installed plugin stays dark until the application starts again. The node half is an empty `apply` that puts the plugin in the Loader; the browser half ships through `exports["./client"]`.

Both halves of the fact belong to the desktop shell, not to this process: the shell stamped the profile manifest while launching this sidecar, and only it can replace the process. The browser half asks over Tauri IPC (`plugins_pending_restart`) through `window.__TAURI__.core.invoke` when that function exists, otherwise `window.__TAURI_INTERNALS__.invoke` — the command function Tauri injects into every WebView. It renders the entry in `sidebar.footer.action` while the answer is true, and on confirmation calls `restart_for_plugins`. It decides nothing and reads no harness state. Outside the packaged application both globals are absent, every call reports "nothing pending", and the entry never renders — which is also what a browser tab on `dsh web` sees, since that surface re-reads its patch layer live and has nothing to restart for.

The confirmation is unconditional rather than gated on a busy session. A restart stops the whole local session process, and the browser has no cross-session view of which sessions hold an answer in flight: `SessionListState` carries ids, summaries, phase, and jobs, but no running flag. Asking once is the honest default; a conditional prompt would have to guess. The dialog states what a restart costs — an answer in progress is interrupted, saved history is not.

The plugin is activated by the shell's own patch layer (`apps/desktop/desktop.patch.yml`, passed as `--patch`), never by the shared web bundle, so `npx @deepseek-ai/dsh web` composes exactly as before.

## Model Experience

None, as the plugin contributes one browser control and no host behavior; nothing here reaches a model request.

#### KV Cache effect

None; this package neither assembles nor sends a provider request.

## Known Limitations and Deferred Work

- **Restart, not hot reload** — a newly installed plugin cannot be mounted into the running page: the client HMR receiver refuses an entry that is not already in its loader tree, `window.__DSH_BOOT__` is injected into the served index so a new entry needs a fresh document, and the root Include entry that would recompose the host tree is private to `dsh-app-boot`. Any one of the three would be enough on its own.
- **The prompt cannot tell a busy session from an idle one** — see above; it asks every time.
- **Polling, not push** — the shell has no event channel to this page, so the entry re-asks every five seconds. A change is visible within that window rather than immediately.
