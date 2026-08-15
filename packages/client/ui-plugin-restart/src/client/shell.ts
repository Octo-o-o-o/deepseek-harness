/**
 * The desktop shell's IPC surface, as seen from the sidecar page.
 *
 * The page is a remote origin to Tauri, so these commands answer only after
 * the shell registers a capability naming this launch's port. Outside the
 * shell — a browser tab on `dsh web` — the global is absent and every call
 * here reports "nothing pending", which keeps the action invisible rather
 * than broken.
 */

/** Commands this surface calls, and what each answers. */
interface ShellCommands {
  plugins_pending_restart: boolean
  restart_for_plugins: void
}

/** Shape of `window.__TAURI__` this module reads; absent outside the shell. */
interface TauriGlobal {
  core?: {
    invoke?: (command: string, args?: Record<string, unknown>) => Promise<unknown>
  }
}

function bridge(): TauriGlobal['core'] | undefined {
  return (globalThis as { __TAURI__?: TauriGlobal }).__TAURI__?.core
}

/**
 * Whether the desktop shell is hosting this page.
 *
 * @returns true when the shell's IPC bridge is reachable.
 */
export function isShellHosted(): boolean {
  return typeof bridge()?.invoke === 'function'
}

/**
 * Ask the shell whether the profile changed since it composed this sidecar.
 *
 * @returns true when a restart would pick up plugins the running composition
 * never read; false outside the shell, and false when the command fails —
 * an unreachable shell must not pin a restart prompt to the sidebar.
 */
export async function pluginsPendingRestart(): Promise<boolean> {
  const invoke = bridge()?.invoke
  if (invoke === undefined) return false
  try {
    return await invoke('plugins_pending_restart') === true
  } catch {
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
  const invoke = bridge()?.invoke
  if (invoke === undefined) throw new Error('plugin restart: the desktop shell is not hosting this page')
  await invoke('restart_for_plugins')
}

export type { ShellCommands }
