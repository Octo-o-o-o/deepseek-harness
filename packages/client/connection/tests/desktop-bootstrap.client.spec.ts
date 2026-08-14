/** Desktop bootstrap helpers: wait for the page POST, then signal ready. */

import { afterEach, describe, expect, it, vi } from 'vitest'
import { awaitDesktopBootstrap, isDesktopShell, signalDesktopReady } from '../src/client/desktop-bootstrap.ts'

type DesktopRoot = {
  __DSH_DESKTOP_BOOTSTRAP__?: unknown
  __DSH_DESKTOP_BOOTSTRAP_DONE__?: unknown
}

afterEach(() => {
  delete (globalThis as DesktopRoot).__DSH_DESKTOP_BOOTSTRAP__
  delete (globalThis as DesktopRoot).__DSH_DESKTOP_BOOTSTRAP_DONE__
  vi.unstubAllGlobals()
})

describe('awaitDesktopBootstrap', () => {
  it('is a no-op without a pending promise and waits when one exists', async () => {
    await expect(awaitDesktopBootstrap()).resolves.toBeUndefined()
    let settled = false
    ;(globalThis as DesktopRoot).__DSH_DESKTOP_BOOTSTRAP_DONE__ = Promise.resolve().then(() => {
      settled = true
    })
    await awaitDesktopBootstrap()
    expect(settled).toBe(true)
  })
})

describe('isDesktopShell', () => {
  it('reads the marker the shell writes into the index', () => {
    expect(isDesktopShell()).toBe(false)
    ;(globalThis as DesktopRoot).__DSH_DESKTOP_BOOTSTRAP__ = ''
    expect(isDesktopShell()).toBe(false)
    ;(globalThis as DesktopRoot).__DSH_DESKTOP_BOOTSTRAP__ = 'nonce'
    expect(isDesktopShell()).toBe(true)
  })
})

describe('signalDesktopReady', () => {
  it('does not fetch when the desktop marker is absent', async () => {
    const fetchMock = vi.fn()
    vi.stubGlobal('fetch', fetchMock)
    await signalDesktopReady()
    expect(fetchMock).not.toHaveBeenCalled()
  })

  it('POSTs /__dshd_ready when the desktop marker is set', async () => {
    ;(globalThis as DesktopRoot).__DSH_DESKTOP_BOOTSTRAP__ = 'nonce'
    const fetchMock = vi.fn(async (input: URL) => {
      expect(input.pathname).toBe('/__dshd_ready')
      return { ok: true }
    })
    vi.stubGlobal('fetch', fetchMock)
    await signalDesktopReady()
    expect(fetchMock).toHaveBeenCalledOnce()
  })
})
