/** Browser desktop bootstrap: wait for the nonce exchange, then signal ready. */

const READY_PATH = '/__dshd_ready'
const INTERNAL_BASE = 'http://dsh.internal'

type DesktopRoot = {
  __DSH_DESKTOP_BOOTSTRAP__?: unknown
  __DSH_DESKTOP_BOOTSTRAP_DONE__?: unknown
}

/**
 * Wait for the page's bootstrap POST when the desktop marker is present.
 *
 * @returns a promise that settles after bootstrap, or immediately otherwise.
 */
export async function awaitDesktopBootstrap(): Promise<void> {
  const pending = (globalThis as DesktopRoot).__DSH_DESKTOP_BOOTSTRAP_DONE__
  if (pending !== undefined && typeof (pending as Promise<unknown>).then === 'function') {
    await pending
  }
}

/**
 * Tell the host this generation's two downlinks are up. No-op outside desktop.
 *
 * @returns a promise that settles after the ready POST, or immediately.
 */
export async function signalDesktopReady(): Promise<void> {
  const mark = (globalThis as DesktopRoot).__DSH_DESKTOP_BOOTSTRAP__
  if (typeof mark !== 'string' || mark === '') return
  const response = await globalThis.fetch(new URL(READY_PATH, resolveBase()), {
    method: 'POST',
    credentials: 'same-origin',
  })
  if (!response.ok) {
    throw new Error(`desktop ready signal failed: HTTP ${response.status}`)
  }
}

function resolveBase(): string {
  const location = (globalThis as { location?: { origin?: string } }).location
  return location?.origin !== undefined && location.origin !== 'null' ? location.origin : INTERNAL_BASE
}
