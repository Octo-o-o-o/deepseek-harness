/**
 * Desktop bootstrap: one-time nonce in the index, HttpOnly cookie for /api.
 * The token itself never appears in HTML or renderer-visible JavaScript.
 */

import { timingSafeEqual } from 'node:crypto'
import type { IncomingMessage, ServerResponse } from 'node:http'
import { hasValidDesktopToken } from '@deepseek-ai/dsh-client-connection/src/api-request-trust.ts'

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

/** Single-use nonce lifetime. */
export const BOOTSTRAP_TTL_MS = 30_000

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
   * @returns whether {@link DesktopBootstrap.markReady} has run.
   */
  isReady(): boolean {
    return this.ready
  }
}

/**
 * Encode a nonce as a JS expression and escape angle brackets so it cannot
 * close the script element.
 *
 * @param nonce - raw bootstrap nonce.
 * @returns a JS expression, including surrounding quotes.
 */
export function encodeBootstrapNonceLiteral(nonce: string): string {
  return JSON.stringify(nonce).replaceAll('<', '\\u003c').replaceAll('>', '\\u003e')
}

/**
 * Inject the desktop bootstrap marker and the onload POST. The token is not
 * written into the page.
 *
 * @param html - index.html body.
 * @param nonce - one-time bootstrap nonce.
 * @returns html with the script inserted after `<head>` or prefixed.
 */
export function injectDesktopBootstrapScript(html: string, nonce: string): string {
  const encoded = encodeBootstrapNonceLiteral(nonce)
  const snippet = `<script>window.${DESKTOP_BOOTSTRAP_MARK}=${encoded};window.${DESKTOP_BOOTSTRAP_DONE}=fetch(${JSON.stringify(BOOTSTRAP_PATH)},{method:"POST",headers:{"content-type":"application/json"},credentials:"same-origin",body:JSON.stringify({nonce:window.${DESKTOP_BOOTSTRAP_MARK}})}).then(function(r){if(!r.ok)throw new Error("desktop bootstrap failed")});</script>`
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
