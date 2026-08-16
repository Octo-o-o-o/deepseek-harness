/**
 * The desktop shell's IPC surface, as seen from the sidecar page.
 *
 * The page is a remote origin to Tauri, so these commands answer only after
 * the shell registers a capability naming this launch's port. Invoke is
 * `window.__TAURI__.core.invoke` when the withGlobalTauri convenience object
 * is present, otherwise `window.__TAURI_INTERNALS__.invoke` — the command
 * function Tauri injects into every WebView and the one `@tauri-apps/api/core`
 * wraps. Outside the shell — a browser tab on `dsh web` — both are absent and
 * every call here reports "nothing pending", which keeps the action invisible
 * rather than broken.
 */

/** Commands this surface calls, and what each answers. */
interface ShellCommands {
  plugins_pending_restart: boolean
  restart_for_plugins: void
}

/** Tauri's injected command function; payload is unused for these two commands. */
type Invoke = (command: string, args?: Record<string, unknown>) => Promise<unknown>

/**
 * Resolve the shell's command function.
 *
 * Prefer the documented withGlobalTauri path; fall back to the injected
 * internals function that `@tauri-apps/api` itself wraps. Either absence is
 * "not the shell", not a pending change.
 *
 * @returns the invoke function, or undefined outside the packaged application.
 */
function resolveInvoke(): Invoke | undefined {
  const coreInvoke = (globalThis as { __TAURI__?: { core?: { invoke?: Invoke } } }).__TAURI__?.core?.invoke
  if (typeof coreInvoke === 'function') {
    return (command, args) => coreInvoke(command, args)
  }
  const internalsInvoke = (globalThis as { __TAURI_INTERNALS__?: { invoke?: Invoke } }).__TAURI_INTERNALS__?.invoke
  if (typeof internalsInvoke === 'function') {
    return (command, args) => internalsInvoke(command, args)
  }
  return undefined
}

/**
 * Whether the desktop shell is hosting this page.
 *
 * @returns true when either injected invoke path is a function.
 */
export function isShellHosted(): boolean {
  return resolveInvoke() !== undefined
}

/**
 * Ask the shell whether the profile changed since it composed this sidecar.
 *
 * @returns true when a restart would pick up plugins the running composition
 * never read; false outside the shell, and false when the command fails —
 * an unreachable shell must not pin a restart prompt to the sidebar.
 */
export async function pluginsPendingRestart(): Promise<boolean> {
  const invoke = resolveInvoke()
  if (invoke === undefined) return false
  try {
    return await invoke('plugins_pending_restart', {}) === true
  } catch {
    // IPC refusal, ACL miss, and a thrown command body all mean the shell
    // cannot be asked; none of those is a pending profile change.
    return false
  }
}

/**
 * Ask the shell to restart the application.
 *
 * The shell replaces its own process, so a resolved promise is not a success
 * signal — the page is gone by then. Rejection means the request never
 * reached the shell.
 *
 * @returns settlement of the IPC call.
 */
export async function requestRestart(): Promise<void> {
  const invoke = resolveInvoke()
  if (invoke === undefined) throw new Error('plugin restart: the desktop shell is not hosting this page')
  await invoke('restart_for_plugins', {})
}

export type { ShellCommands }
