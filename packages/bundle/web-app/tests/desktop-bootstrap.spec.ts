/**
 * Desktop bootstrap session: nonce consume, cookie issuance, status,
 * script-element safety of the injected nonce literal, and the server-wide
 * `/api` guard plus its foreign-route report.
 */

import { createServer } from 'node:http'
import type { IncomingMessage } from 'node:http'
import vm from 'node:vm'
import { describe, expect, it } from 'vitest'
import {
  BOOTSTRAP_FRAGMENT_KEY,
  BOOTSTRAP_PATH,
  BOOTSTRAP_TTL_MS,
  DesktopBootstrap,
  describeForeignApiRoutes,
  desktopApiGuard,
  handleDesktopBootstrap,
  handleDesktopReady,
  handleDesktopStatus,
  injectDesktopBootstrapScript,
  READY_PATH,
  STATUS_PATH,
} from '../src/desktop-bootstrap.ts'

function evaluateInjection(
  html: string,
  hash = '',
): { alertCalls: number; bootstrap: unknown; rewrittenUrl: string | undefined } {
  const script = /<script>([\s\S]*?)<\/script>/.exec(html)?.[1]
  if (script === undefined) throw new Error('missing injected script')
  let alertCalls = 0
  let rewrittenUrl: string | undefined
  const window: Record<string, unknown> = {}
  vm.runInNewContext(script, {
    window,
    URLSearchParams,
    location: { hash, pathname: '/', search: '' },
    history: {
      replaceState: (_state: unknown, _title: unknown, url: string) => { rewrittenUrl = url },
    },
    fetch: async () => ({ ok: true }),
    alert: () => { alertCalls += 1 },
  })
  return { alertCalls, bootstrap: window.__DSH_DESKTOP_BOOTSTRAP__, rewrittenUrl }
}

describe('desktop /api guard', () => {
  const guard = desktopApiGuard(new DesktopBootstrap('tok', 'nonce'))
  const requestWith = (headers: Record<string, string>): IncomingMessage =>
    ({ headers } as unknown as IncomingMessage)

  it('requires the launch token on every /api path, including one no harness plugin registered', () => {
    expect(guard(requestWith({}), '/api/usage-stats/balance')).toBe(401)
    expect(guard(requestWith({ cookie: 'dsh-token=tok' }), '/api/usage-stats/balance')).toBeUndefined()
    expect(guard(requestWith({ 'x-dsh-token': 'tok' }), '/api/host.describe')).toBeUndefined()
    expect(guard(requestWith({ cookie: 'dsh-token=other' }), '/api/host.describe')).toBe(401)
    expect(guard(requestWith({}), '/api')).toBe(401)
  })

  it('admits everything outside the namespace, so the bootstrap exchange still works', () => {
    // The route that issues the cookie cannot present one, and the status
    // route authenticates with the nonce header instead.
    expect(guard(requestWith({}), BOOTSTRAP_PATH)).toBeUndefined()
    expect(guard(requestWith({}), STATUS_PATH)).toBeUndefined()
    expect(guard(requestWith({}), '/')).toBeUndefined()
    expect(guard(requestWith({}), '/apiary')).toBeUndefined()
  })
})

describe('foreign /api route report', () => {
  it('stays silent for a connection-only composition and names every other owner', () => {
    expect(describeForeignApiRoutes([
      { kind: 'prefix', path: '/api', owner: 'client-connection' },
      { kind: 'upgrade', path: '/api/events.mux', owner: 'client-connection' },
      { kind: 'upgrade', path: '/api/events.host', owner: 'client-connection' },
      { kind: 'fallback', path: '*', owner: 'frontend-static' },
    ])).toBeUndefined()
    expect(describeForeignApiRoutes([
      { kind: 'prefix', path: '/api', owner: 'client-connection' },
      { kind: 'exact', path: '/api/host.describe', owner: 'shadow' },
      { kind: 'prefix', path: '/api/internal', owner: 'unknown' },
      { kind: 'upgrade', path: '/api/events.other', owner: 'patch' },
    ])).toBe(
      'web-app: /api routes owned by other plugins are served ahead of the connection prefix '
      + '(exact /api/host.describe owner=shadow, prefix /api/internal owner=unknown, upgrade /api/events.other owner=patch); '
      + 'the desktop launch token still gates them, but an exact path matching an RPC method would replace it',
    )
  })
})

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
  it('takes the nonce from the fragment, so the served index carries no secret', () => {
    const html = injectDesktopBootstrapScript('<head></head>')
    // The served bytes are identical whatever nonce the launch uses: reaching
    // the loopback port yields nothing an attacker could exchange for a cookie.
    expect(html).toBe(injectDesktopBootstrapScript('<head></head>'))
    expect(html).not.toContain('9f86d081884c7d65')
    const { bootstrap } = evaluateInjection(html, `#${BOOTSTRAP_FRAGMENT_KEY}=9f86d081884c7d65`)
    expect(bootstrap).toBe('9f86d081884c7d65')
    expect(html).not.toContain('__DSH_TOKEN__')
  })

  it('strips the consumed nonce from session history', () => {
    const html = injectDesktopBootstrapScript('<head></head>')
    const { rewrittenUrl } = evaluateInjection(html, `#${BOOTSTRAP_FRAGMENT_KEY}=abc123`)
    // No fragment survives, so a back/forward entry and any later page script
    // cannot recover the nonce.
    expect(rewrittenUrl).toBe('/')
  })

  it('preserves unrelated fragment state while removing only the nonce', () => {
    const html = injectDesktopBootstrapScript('<head></head>')
    const { bootstrap, rewrittenUrl } = evaluateInjection(
      html,
      `#view=trajectory&${BOOTSTRAP_FRAGMENT_KEY}=abc123`,
    )
    expect(bootstrap).toBe('abc123')
    expect(rewrittenUrl).toBe('/#view=trajectory')
  })

  it('yields an empty nonce without a fragment, so a shell-less page gets no cookie', () => {
    const html = injectDesktopBootstrapScript('<head></head>')
    const { bootstrap, rewrittenUrl } = evaluateInjection(html, '')
    expect(bootstrap).toBe('')
    expect(rewrittenUrl).toBeUndefined()
  })

  it('treats hostile fragment content as an opaque value rather than code', () => {
    const hostile = "x'*alert(1)*'</script><script>alert(1)"
    const html = injectDesktopBootstrapScript('<head></head>')
    const { alertCalls, bootstrap } = evaluateInjection(
      html,
      `#${BOOTSTRAP_FRAGMENT_KEY}=${encodeURIComponent(hostile)}`,
    )
    expect(alertCalls).toBe(0)
    expect(bootstrap).toBe(hostile)
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
