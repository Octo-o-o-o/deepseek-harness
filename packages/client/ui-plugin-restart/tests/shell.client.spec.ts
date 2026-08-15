/**
 * The shell IPC surface as the page sees it: absent outside the packaged
 * application, and never allowed to turn a shell-side failure into a restart
 * prompt the person cannot act on.
 */
import { afterEach, describe, expect, it } from 'vitest'
import { isShellHosted, pluginsPendingRestart, requestRestart } from '../src/client/shell.ts'

type TauriHost = { __TAURI__?: { core?: { invoke?: (command: string) => Promise<unknown> } } }

function install(invoke: ((command: string) => Promise<unknown>) | undefined): void {
  ;(globalThis as TauriHost).__TAURI__ = invoke === undefined ? {} : { core: { invoke } }
}

afterEach(() => {
  delete (globalThis as TauriHost).__TAURI__
})

describe('shell bridge', () => {
  it('reports no shell when the global is missing or carries no invoke', () => {
    expect(isShellHosted()).toBe(false)
    install(undefined)
    expect(isShellHosted()).toBe(false)
    install(async () => true)
    expect(isShellHosted()).toBe(true)
  })

  it('answers "nothing pending" outside the shell rather than throwing', async () => {
    await expect(pluginsPendingRestart()).resolves.toBe(false)
  })

  it('reads the shell answer and treats anything but true as nothing pending', async () => {
    install(async () => true)
    await expect(pluginsPendingRestart()).resolves.toBe(true)
    install(async () => false)
    await expect(pluginsPendingRestart()).resolves.toBe(false)
    // A shell that answers something unexpected must not pin a banner to the
    // sidebar, so only an exact `true` counts.
    install(async () => 'yes')
    await expect(pluginsPendingRestart()).resolves.toBe(false)
  })

  it('swallows a failing command: an unreachable shell is not a pending change', async () => {
    install(() => Promise.reject(new Error('ipc down')))
    await expect(pluginsPendingRestart()).resolves.toBe(false)
  })

  it('refuses to request a restart with no shell, and forwards it when there is one', async () => {
    await expect(requestRestart()).rejects.toThrow(/desktop shell is not hosting/)
    const seen: string[] = []
    install(async (command) => {
      seen.push(command)
      return undefined
    })
    await requestRestart()
    expect(seen).toEqual(['restart_for_plugins'])
  })

  it('propagates a rejected restart so the surface can say it failed', async () => {
    install(() => Promise.reject(new Error('denied')))
    await expect(requestRestart()).rejects.toThrow(/denied/)
  })
})
