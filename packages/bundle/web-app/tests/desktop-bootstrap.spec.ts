/**
 * Desktop bootstrap session: nonce consume, cookie issuance, status, and
 * script-element safety of the injected nonce literal.
 */

import { createServer } from 'node:http'
import vm from 'node:vm'
import { describe, expect, it } from 'vitest'
import {
  BOOTSTRAP_PATH,
  BOOTSTRAP_TTL_MS,
  DesktopBootstrap,
  encodeBootstrapNonceLiteral,
  handleDesktopBootstrap,
  handleDesktopReady,
  handleDesktopStatus,
  injectDesktopBootstrapScript,
  READY_PATH,
  STATUS_PATH,
} from '../src/desktop-bootstrap.ts'

function evaluateInjection(html: string): { alertCalls: number; bootstrap: unknown } {
  const script = /<script>([\s\S]*?)<\/script>/.exec(html)?.[1]
  if (script === undefined) throw new Error('missing injected script')
  let alertCalls = 0
  const window: Record<string, unknown> = {}
  vm.runInNewContext(script, {
    window,
    fetch: async () => ({ ok: true }),
    alert: () => { alertCalls += 1 },
  })
  return { alertCalls, bootstrap: window.__DSH_DESKTOP_BOOTSTRAP__ }
}

describe('nonce consume', () => {
  it('accepts the nonce once and rejects reuse, mismatch, and expiry', () => {
    const session = new DesktopBootstrap('tok', 'nonce', BOOTSTRAP_TTL_MS, 1_000)
    expect(session.consume('wrong', 1_001)).toBe('invalid')
    expect(session.consume('nonce', 1_000 + BOOTSTRAP_TTL_MS)).toBe('expired')
    expect(session.consume('nonce', 1_001)).toBe('ok')
    expect(session.consume('nonce', 1_002)).toBe('used')
  })
})

describe('script injection', () => {
  it('JSON-encodes quotes and angle brackets so evaluation cannot run attacker code', () => {
    const hostile = "x'*alert(1)*'</script><script>alert(1)"
    const html = injectDesktopBootstrapScript('<head></head>', hostile)
    expect(html).not.toContain(hostile)
    expect(encodeBootstrapNonceLiteral(hostile)).toContain('\\u003c')
    const { alertCalls, bootstrap } = evaluateInjection(html)
    expect(alertCalls).toBe(0)
    expect(bootstrap).toBe(hostile)
    expect(html).not.toContain('__DSH_TOKEN__')
  })
})

describe('HTTP routes', () => {
  it('issues the HttpOnly cookie once and then rejects the spent nonce', async () => {
    const session = new DesktopBootstrap('tok', 'nonce')
    const server = createServer((req, res) => {
      if (req.url === BOOTSTRAP_PATH) {
        void handleDesktopBootstrap(req, res, session)
        return
      }
      if (req.url === READY_PATH) {
        handleDesktopReady(req, res, session)
        return
      }
      handleDesktopStatus(req, res, session)
    })
    await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve))
    const address = server.address()
    if (address === null || typeof address === 'string') throw new Error('expected tcp address')
    const base = `http://127.0.0.1:${String(address.port)}`
    try {
      const first = await fetch(`${base}${BOOTSTRAP_PATH}`, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ nonce: 'nonce' }),
      })
      expect(first.status).toBe(204)
      const cookies = first.headers.getSetCookie()
      expect(cookies.some(entry => entry.includes('dsh-token=tok') && entry.includes('Path=/api') && entry.includes('HttpOnly'))).toBe(true)
      expect(cookies.some(entry => entry.includes(`Path=${READY_PATH}`) && entry.includes('HttpOnly'))).toBe(true)
      expect(first.headers.get('cache-control')).toBe('no-store')

      const replay = await fetch(`${base}${BOOTSTRAP_PATH}`, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ nonce: 'nonce' }),
      })
      expect(replay.status).toBe(401)

      const statusUnauthorized = await fetch(`${base}${STATUS_PATH}`)
      expect(statusUnauthorized.status).toBe(401)
      const status = await fetch(`${base}${STATUS_PATH}`, { headers: { 'x-dsh-bootstrap': 'nonce' } })
      expect(status.status).toBe(200)
      expect(await status.json()).toEqual({ ready: false })

      const readyDenied = await fetch(`${base}${READY_PATH}`, { method: 'POST' })
      expect(readyDenied.status).toBe(401)
      const ready = await fetch(`${base}${READY_PATH}`, {
        method: 'POST',
        headers: { cookie: 'dsh-token=tok' },
      })
      expect(ready.status).toBe(204)
      const readyAgain = await fetch(`${base}${STATUS_PATH}`, { headers: { 'x-dsh-bootstrap': 'nonce' } })
      expect(await readyAgain.json()).toEqual({ ready: true })
    } finally {
      await new Promise<void>((resolve, reject) => {
        server.close((error) => { if (error !== undefined) reject(error); else resolve() })
      })
    }
  })
})
