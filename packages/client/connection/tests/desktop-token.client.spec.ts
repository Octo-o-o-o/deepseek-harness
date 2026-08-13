/** Page-token helper: fetch/RPC attach X-DSH-Token only when the page injected one. */

import { afterEach, describe, expect, it } from 'vitest'
import { DESKTOP_TOKEN_HEADER, readPageDesktopToken, withDesktopToken } from '../src/client/desktop-token.ts'

type TokenRoot = { __DSH_TOKEN__?: unknown; window?: { __DSH_TOKEN__?: unknown } }

afterEach(() => {
  delete (globalThis as TokenRoot).__DSH_TOKEN__
  delete (globalThis as TokenRoot).window
})

describe('readPageDesktopToken', () => {
  it('reads a non-empty global or window token and ignores everything else', () => {
    expect(readPageDesktopToken()).toBeUndefined()
    ;(globalThis as TokenRoot).__DSH_TOKEN__ = ''
    expect(readPageDesktopToken()).toBeUndefined()
    ;(globalThis as TokenRoot).__DSH_TOKEN__ = 1
    expect(readPageDesktopToken()).toBeUndefined()
    ;(globalThis as TokenRoot).__DSH_TOKEN__ = 'abc'
    expect(readPageDesktopToken()).toBe('abc')
    delete (globalThis as TokenRoot).__DSH_TOKEN__
    ;(globalThis as TokenRoot).window = { __DSH_TOKEN__: 'from-window' }
    expect(readPageDesktopToken()).toBe('from-window')
  })
})

describe('withDesktopToken', () => {
  it('returns the original init when no token is present', () => {
    expect(withDesktopToken()).toEqual({})
    const init = { method: 'POST' }
    expect(withDesktopToken(init)).toBe(init)
  })

  it('adds X-DSH-Token without dropping existing headers', () => {
    ;(globalThis as TokenRoot).__DSH_TOKEN__ = 'secret'
    const next = withDesktopToken({ headers: { 'content-type': 'application/json' } })
    const headers = new Headers(next.headers)
    expect(headers.get(DESKTOP_TOKEN_HEADER)).toBe('secret')
    expect(headers.get('content-type')).toBe('application/json')
  })
})
