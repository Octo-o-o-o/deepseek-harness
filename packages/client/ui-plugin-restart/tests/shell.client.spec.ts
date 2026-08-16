/**
 * The shell IPC surface as the page sees it: absent outside the packaged
 * application, reachable through either injected invoke path, and never
 * allowed to turn a shell-side failure into a restart prompt the person
 * cannot act on.
 */
import { afterEach, describe, expect, it } from 'vitest'
import { isShellHosted, pluginsPendingRestart, requestRestart } from '../src/client/shell.ts'

type Invoke = (command: string, args?: Record<string, unknown>) => Promise<unknown>

type Host = {
  __TAURI__?: { core?: { invoke?: Invoke } }
  __TAURI_INTERNALS__?: { invoke?: Invoke }
}

function installCore(invoke: Invoke | undefined): void {
  ;(globalThis as Host).__TAURI__ = invoke === undefined ? {} : { core: { invoke } }
}

function installInternals(invoke: Invoke): void {
  ;(globalThis as Host).__TAURI_INTERNALS__ = { invoke }
}

afterEach(() => {
  delete (globalThis as Host).__TAURI__
  delete (globalThis as Host).__TAURI_INTERNALS__
})

describe('shell bridge', () => {
  it('reports no shell when both injected invoke paths are missing', () => {
    expect(isShellHosted()).toBe(false)
    installCore(undefined)
    expect(isShellHosted()).toBe(false)
    ;(globalThis as Host).__TAURI__ = { core: {} }
    expect(isShellHosted()).toBe(false)
    ;(globalThis as Host).__TAURI_INTERNALS__ = {}
    expect(isShellHosted()).toBe(false)
  })

  it('treats the withGlobalTauri convenience invoke as a hosted shell', () => {
    installCore(async () => true)
    expect(isShellHosted()).toBe(true)
  })

  it('treats Tauri\'s injected internals invoke as a hosted shell', () => {
    installInternals(async () => true)
    expect(isShellHosted()).toBe(true)
  })

  it('answers "nothing pending" outside the shell rather than throwing', async () => {
    await expect(pluginsPendingRestart()).resolves.toBe(false)
  })

  it('reads the shell answer and treats anything but true as nothing pending', async () => {
    installCore(async () => true)
    await expect(pluginsPendingRestart()).resolves.toBe(true)
    installCore(async () => false)
    await expect(pluginsPendingRestart()).resolves.toBe(false)
    // A shell that answers something unexpected must not pin a banner to the
    // sidebar, so only an exact `true` counts.
    installCore(async () => 'yes')
    await expect(pluginsPendingRestart()).resolves.toBe(false)
  })

  it('reads a pending change through internals when the convenience object is absent', async () => {
    installInternals(async () => true)
    await expect(pluginsPendingRestart()).resolves.toBe(true)
  })

  it('prefers the withGlobalTauri convenience invoke when both paths exist', async () => {
    const seen: string[] = []
    installCore(async (command) => {
      seen.push(`core:${command}`)
      return true
    })
    installInternals(async (command) => {
      seen.push(`internals:${command}`)
      return false
    })
    await expect(pluginsPendingRestart()).resolves.toBe(true)
    expect(seen).toEqual(['core:plugins_pending_restart'])
  })

  it('swallows a failing command: an unreachable shell is not a pending change', async () => {
    installCore(() => Promise.reject(new Error('ipc down')))
    await expect(pluginsPendingRestart()).resolves.toBe(false)
  })

  it('refuses to request a restart with no shell, and forwards it when there is one', async () => {
    await expect(requestRestart()).rejects.toThrow(/desktop shell is not hosting/)
    const seen: string[] = []
    installCore(async (command) => {
      seen.push(command)
      return undefined
    })
    await requestRestart()
    expect(seen).toEqual(['restart_for_plugins'])
  })

  it('forwards a restart through internals when the convenience object is absent', async () => {
    const seen: string[] = []
    installInternals(async (command) => {
      seen.push(command)
      return undefined
    })
    await requestRestart()
    expect(seen).toEqual(['restart_for_plugins'])
  })

  it('propagates a rejected restart so the surface can say it failed', async () => {
    installCore(() => Promise.reject(new Error('denied')))
    await expect(requestRestart()).rejects.toThrow(/denied/)
  })
})
