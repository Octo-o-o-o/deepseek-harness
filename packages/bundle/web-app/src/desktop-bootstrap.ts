/**
 * Desktop bootstrap: one-time nonce delivered through the URL fragment,
 * HttpOnly cookie for /api. Neither the token nor the nonce appears in any
 * response body, so reaching the loopback port is not enough to obtain one.
 */

import { timingSafeEqual } from 'node:crypto'
import type { IncomingMessage, ServerResponse } from 'node:http'
import { hasValidDesktopToken } from '@deepseek-ai/dsh-client-connection'

/** Index global holding the one-time bootstrap nonce. */
export const DESKTOP_BOOTSTRAP_MARK = '__DSH_DESKTOP_BOOTSTRAP__'

/** Index global holding the in-flight bootstrap fetch. */
export const DESKTOP_BOOTSTRAP_DONE = '__DSH_DESKTOP_BOOTSTRAP_DONE__'

/** Exact route that exchanges a nonce for the HttpOnly token cookie. */
export const BOOTSTRAP_PATH = '/__dshd_bootstrap'

/** Exact route the browser client posts after both downlinks are up. */
export const READY_PATH = '/__dshd_ready'

/** Exact route the desktop shell polls with the bootstrap nonce. */
export const STATUS_PATH = '/__dshd_status'

/** Header the shell sends when polling {@link STATUS_PATH}. */
export const BOOTSTRAP_STATUS_HEADER = 'x-dsh-bootstrap'

/** Owner id the connection plugin writes on `/api` registrations. */
export const CONNECTION_ROUTE_OWNER = 'client-connection'

/** Single-use nonce lifetime. Spans the desktop shell's whole boot budget —
 * sidecar cold start (first launch can exceed 15s under real-time AV scans),
 * health checks, WebView navigation, and the page's first parse — so it is
 * minutes, not seconds; single-use keeps the longer window cheap. */
export const BOOTSTRAP_TTL_MS = 120_000

/** Why {@link DesktopBootstrap.consume} refused a nonce. */
export type ConsumeFailure = 'invalid' | 'expired' | 'used'

const BOOTSTRAP_BODY_LIMIT = 4096

/**
 * In-memory desktop bootstrap session for one sidecar launch.
 */
export class DesktopBootstrap {
  private consumed = false
  private ready = false

  /**
   * @param token - per-launch token written only into Set-Cookie.
   * @param nonce - one-time nonce injected into the index.
   * @param ttlMs - consume window from {@link DesktopBootstrap.createdAt}.
   * @param createdAt - epoch ms when the session started.
   */
  constructor(
    readonly token: string,
    readonly nonce: string,
    readonly ttlMs = BOOTSTRAP_TTL_MS,
    readonly createdAt = Date.now(),
  ) {}

  /**
   * Consume the nonce exactly once inside the TTL.
   *
   * @param presented - nonce from the bootstrap POST body.
   * @param now - epoch ms, injectable in tests.
   * @returns `ok` or the refusal reason.
   */
  consume(presented: string, now = Date.now()): 'ok' | ConsumeFailure {
    if (!secretsEqual(presented, this.nonce)) return 'invalid'
    if (this.consumed) return 'used'
    if (now - this.createdAt >= this.ttlMs) return 'expired'
    this.consumed = true
    return 'ok'
  }

  /**
   * Compare `X-DSH-Bootstrap` without consuming the nonce.
   *
   * @param value - header value.
   * @returns true when it matches the live nonce.
   */
  matchesBootstrapHeader(value: string | undefined): boolean {
    return value !== undefined && secretsEqual(value, this.nonce)
  }

  /** Record that the browser client finished its two-stream handshake. */
  markReady(): void {
    this.ready = true
  }

  /**
   * Report the browser client's handshake state for this launch.
   * @returns whether {@link DesktopBootstrap.markReady} has run.
   */
  isReady(): boolean {
    return this.ready
  }
}

/**
 * URL fragment key carrying the one-time nonce from the shell to the index.
 *
 * The fragment is the delivery channel precisely because user agents never put
 * it on the wire: the shell navigates to `…/#dshd-nonce=<nonce>`, so the nonce
 * reaches the page without appearing in any response body. Serving it inside
 * the index instead would hand it to every local process that can reach the
 * loopback port, and loopback carries no user identity.
 */
export const BOOTSTRAP_FRAGMENT_KEY = 'dshd-nonce'

/**
 * Inject the desktop bootstrap reader and the onload POST. Neither the token
 * nor the nonce is written into the page: the script reads the nonce from the
 * URL fragment the shell navigated to, then strips it from session history so
 * it does not survive in the back/forward entry or reach later page scripts.
 *
 * An absent fragment yields an empty nonce, which `/__dshd_bootstrap` rejects —
 * a page opened without the shell gets no cookie rather than a partial session.
 *
 * @param html - index.html body.
 * @returns html with the script inserted after `<head>` or prefixed.
 */
export function injectDesktopBootstrapScript(html: string): string {
  const key = JSON.stringify(BOOTSTRAP_FRAGMENT_KEY)
  const read = '(function(){var h=location.hash;if(h.charAt(0)==="#")h=h.slice(1);'
    + `var p=new URLSearchParams(h);var v=p.get(${key});if(v===null)return"";`
    + `p.delete(${key});var rest=p.toString();`
    + 'history.replaceState(null,"",location.pathname+location.search+(rest===""?"":"#"+rest));'
    + 'return v;})()'
  const snippet = `<script>window.${DESKTOP_BOOTSTRAP_MARK}=${read};window.${DESKTOP_BOOTSTRAP_DONE}=fetch(${JSON.stringify(BOOTSTRAP_PATH)},{method:"POST",headers:{"content-type":"application/json"},credentials:"same-origin",body:JSON.stringify({nonce:window.${DESKTOP_BOOTSTRAP_MARK}})}).then(function(r){if(!r.ok)throw new Error("desktop bootstrap failed")});</script>`
  const head = html.indexOf('<head>')
  if (head === -1) return `${snippet}${html}`
  const insertAt = head + '<head>'.length
  return `${html.slice(0, insertAt)}${snippet}${html.slice(insertAt)}`
}

/**
 * POST `/__dshd_bootstrap`: consume the nonce and set the HttpOnly cookie.
 *
 * @param req - incoming request.
 * @param res - response to write.
 * @param session - live bootstrap session.
 * @returns a promise that settles after the response is written.
 */
export async function handleDesktopBootstrap(
  req: IncomingMessage,
  res: ServerResponse,
  session: DesktopBootstrap,
): Promise<void> {
  if (req.method !== 'POST') {
    writeEmpty(res, 405, { allow: 'POST' })
    return
  }
  let body: string
  try {
    body = await readLimitedBody(req, BOOTSTRAP_BODY_LIMIT)
  } catch {
    writeEmpty(res, 413)
    return
  }
  let nonce: unknown
  try {
    nonce = (JSON.parse(body) as { nonce?: unknown }).nonce
  } catch {
    writeEmpty(res, 400)
    return
  }
  if (typeof nonce !== 'string' || nonce === '') {
    writeEmpty(res, 400)
    return
  }
  const result = session.consume(nonce)
  if (result !== 'ok') {
    writeEmpty(res, 401)
    return
  }
  const cookie = encodeURIComponent(session.token)
  res.writeHead(204, {
    'cache-control': 'no-store',
    'set-cookie': [
      `dsh-token=${cookie}; Path=/api; HttpOnly; SameSite=Strict`,
      `dsh-token=${cookie}; Path=${READY_PATH}; HttpOnly; SameSite=Strict`,
    ],
  })
  res.end()
}

/**
 * POST `/__dshd_ready`: require the token cookie, then mark the client ready.
 *
 * @param req - incoming request.
 * @param res - response to write.
 * @param session - live bootstrap session.
 */
export function handleDesktopReady(
  req: IncomingMessage,
  res: ServerResponse,
  session: DesktopBootstrap,
): void {
  if (req.method !== 'POST') {
    writeEmpty(res, 405, { allow: 'POST' })
    return
  }
  if (!hasValidDesktopToken(req, session.token)) {
    writeEmpty(res, 401)
    return
  }
  session.markReady()
  writeEmpty(res, 204)
}

/**
 * GET `/__dshd_status`: nonce header, JSON `{ready}`.
 *
 * @param req - incoming request.
 * @param res - response to write.
 * @param session - live bootstrap session.
 */
export function handleDesktopStatus(
  req: IncomingMessage,
  res: ServerResponse,
  session: DesktopBootstrap,
): void {
  if (req.method !== 'GET') {
    writeEmpty(res, 405, { allow: 'GET' })
    return
  }
  const header = headerValue(req.headers[BOOTSTRAP_STATUS_HEADER])
  if (!session.matchesBootstrapHeader(header)) {
    writeEmpty(res, 401)
    return
  }
  const body = JSON.stringify({ ready: session.isReady() })
  res.writeHead(200, {
    'cache-control': 'no-store',
    'content-type': 'application/json',
    'content-length': Buffer.byteLength(body),
  })
  res.end(body)
}

function secretsEqual(left: string, right: string): boolean {
  const a = Buffer.from(left)
  const b = Buffer.from(right)
  if (a.length !== b.length) return false
  return timingSafeEqual(a, b)
}

function headerValue(value: string | string[] | undefined): string | undefined {
  if (typeof value === 'string' && value !== '') return value
  return undefined
}

function writeEmpty(
  res: ServerResponse,
  status: number,
  extra: Record<string, string> = {},
): void {
  res.writeHead(status, { 'cache-control': 'no-store', ...extra })
  res.end()
}

/**
 * Whether a registered path sits in the `/api` namespace.
 *
 * @param path - route pathname.
 * @returns true for `/api` and `/api/...`.
 */
export function isApiNamespace(path: string): boolean {
  return path === '/api' || path.startsWith('/api/')
}

/**
 * Fail closed when anything other than connection owns an `/api` route.
 *
 * @param registrations - snapshot from `webServer.listRegistrations()`.
 */
export function assertDesktopApiExclusive(
  registrations: readonly { kind: string; path: string; owner: string }[],
): void {
  const offenders = registrations.filter(entry =>
    (entry.kind === 'exact' || entry.kind === 'prefix' || entry.kind === 'upgrade')
    && isApiNamespace(entry.path)
    && entry.owner !== CONNECTION_ROUTE_OWNER,
  )
  if (offenders.length === 0) return
  const detail = offenders
    .map(entry => `${entry.kind} ${entry.path} owner=${entry.owner}`)
    .join(', ')
  throw new Error(`web-app: desktop mode refuses extra /api registrations: ${detail}`)
}

function readLimitedBody(req: IncomingMessage, limit: number): Promise<string> {
  return new Promise((resolve, reject) => {
    const chunks: Buffer[] = []
    let size = 0
    req.on('data', (chunk: Buffer | string) => {
      const buf = Buffer.from(chunk)
      size += buf.length
      if (size > limit) {
        req.destroy()
        reject(new Error('body too large'))
        return
      }
      chunks.push(buf)
    })
    req.on('end', () => { resolve(Buffer.concat(chunks).toString('utf8')) })
    req.on('error', reject)
  })
}
