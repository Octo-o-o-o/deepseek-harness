/**
 * Desktop-only share gateway: a separate http.Server that holds revocable
 * `dsh-share` sessions and reverse-proxies the loopback sidecar. Installed
 * only when the paired desktop env is set. External browsers never receive
 * the launch token; the gateway injects it on the hop to 127.0.0.1.
 * @module
 */

import { randomBytes, timingSafeEqual } from 'node:crypto'
import { createServer } from 'node:http'
import type { IncomingHttpHeaders, IncomingMessage, Server, ServerResponse } from 'node:http'
import { request as httpRequest } from 'node:http'
import { networkInterfaces } from 'node:os'
import type { NetworkInterfaceInfo } from 'node:os'
import type { Duplex } from 'node:stream'
import type { Context } from '@deepseek-ai/cordis'
import type { DesktopBootstrap } from './desktop-bootstrap.ts'

/** Hostname the gateway writes on remote hops so privileged methods stay 403. */
export const SHARE_INTERNAL_HOST = 'dshd.share.internal'

/** Exact route the shell POSTs with the launch-token header. */
export const SHARE_CONTROL_PATH = '/__dshd_share'

/** Cookie the gateway issues to paired browsers. */
export const SHARE_COOKIE = 'dsh-share'

/** Pairing URL prefix. The webserver has no parameterized routes. */
export const PAIR_PREFIX = '/p'

/** Loopback pairing window after Open in Browser. */
export const LOOPBACK_PAIR_MS = 12_000

/** One-time ticket lifetime. */
export const TICKET_TTL_MS = 180_000

const CONTROL_BODY_LIMIT = 4096
const SIDECAR_HOST = '127.0.0.1'

const STRIP_ALWAYS = new Set([
  'host',
  'origin',
  'cookie',
  'x-dsh-token',
  'x-dsh-bootstrap',
  'x-forwarded-for',
  'x-forwarded-host',
  'x-forwarded-proto',
  'x-forwarded-server',
  'x-real-ip',
  'forwarded',
])

const STRIP_HTTP = new Set([
  'connection',
  'keep-alive',
  'proxy-authenticate',
  'proxy-authorization',
  'te',
  'trailers',
  'transfer-encoding',
  'upgrade',
])

const DOCKER_IFACE = /^(docker|br-|veth|cni|flannel|cbr)/i

/** One non-internal IPv4 the nearby QR may use. */
export interface ShareAddress {
  /** IPv4 literal. */
  address: string
  /** OS interface name. */
  iface: string
}

/** Snapshot the shell's share window renders. */
export interface ShareStatus {
  /** Loopback gateway port, or null before listen finishes. */
  loopbackPort: number | null
  /** Nearby listen, or null when off. */
  nearby: { bindAddress: string; port: number } | null
  /** MagicDNS Host the Tailscale hop must present, or null when off. */
  tailscaleAudience: string | null
  /** Candidate LAN addresses for the nearby QR. */
  addresses: ShareAddress[]
  /** Nearby pairing URL including the live ticket, or null. */
  nearbyTicketUrl: string | null
  /** Tailscale pairing URL including the live ticket, or null. */
  tailscaleTicketUrl: string | null
}

type SessionKind = 'loopback' | 'nearby' | 'tailscale'
type ListenerKind = 'loopback' | 'nearby'

interface ShareSession {
  id: string
  kind: SessionKind
  generation: number
}

interface Ticket {
  id: string
  kind: 'nearby' | 'tailscale'
  audienceHost: string
  expiresAt: number
  consumed: boolean
}

interface GatewayOptions {
  /** Per-launch token injected on the sidecar hop. */
  launchToken: string
  /** Loopback sidecar port. */
  sidecarPort: number
  /** Clock, injectable in tests. */
  now?: () => number
  /** Interface table, injectable in tests. */
  interfaces?: NodeJS.Dict<NetworkInterfaceInfo[]>
}

/**
 * List IPv4 addresses the nearby QR may advertise. Skips internal, Docker, and
 * link-local addresses. RFC1918 Wi-Fi literals sort first.
 *
 * @param ifaces - `os.networkInterfaces()` table.
 * @returns addresses the share window can select.
 */
export function listShareAddresses(
  ifaces: NodeJS.Dict<NetworkInterfaceInfo[]> = networkInterfaces(),
): ShareAddress[] {
  const out: ShareAddress[] = []
  for (const [iface, entries] of Object.entries(ifaces)) {
    if (entries === undefined) continue
    for (const entry of entries) {
      if (entry.family !== 'IPv4') continue
      if (entry.internal) continue
      if (DOCKER_IFACE.test(iface) || isDockerBridge(entry.address) || isLinkLocal(entry.address)) continue
      out.push({ address: entry.address, iface })
    }
  }
  out.sort((left, right) => {
    const delta = shareAddressScore(right) - shareAddressScore(left)
    return delta !== 0 ? delta : left.address.localeCompare(right.address)
  })
  return out
}

/**
 * Desktop share gateway: loopback listen always, nearby listen on demand.
 */
export class ShareGateway {
  private readonly launchToken: string
  private readonly sidecarPort: number
  private readonly now: () => number
  private readonly interfaces: NodeJS.Dict<NetworkInterfaceInfo[]> | undefined
  private disposed = false
  private loopbackServer: Server | undefined
  private nearbyServer: Server | undefined
  private loopbackPort: number | undefined
  private nearbyBind: string | undefined
  private nearbyPort: number | undefined
  private nearbyGeneration = 1
  private tailscaleGeneration = 1
  private tailscaleAudience: string | undefined
  private loopbackPairUntil = 0
  private readonly sessions = new Map<string, ShareSession>()
  private readonly tickets = new Map<string, Ticket>()
  private readonly loopbackSockets = new Set<Duplex>()
  private readonly nearbySockets = new Set<Duplex>()

  /**
   * @param options - launch token, sidecar port, and test hooks.
   */
  constructor(options: GatewayOptions) {
    this.launchToken = options.launchToken
    this.sidecarPort = options.sidecarPort
    this.now = options.now ?? Date.now
    this.interfaces = options.interfaces
  }

  /**
   * Listen on 127.0.0.1 with an OS-assigned port.
   *
   * @returns the bound loopback port.
   */
  async listenLoopback(): Promise<number> {
    if (this.disposed) throw new Error('share gateway: disposed')
    if (this.loopbackPort !== undefined) return this.loopbackPort
    const server = this.createListener('loopback')
    const port = await listen(server, SIDECAR_HOST)
    /* v8 ignore next 4 -- dispose racing listen is covered by the disposed-before-listen throw */
    if (this.disposed) {
      await closeServer(server, this.loopbackSockets)
      throw new Error('share gateway: disposed before loopback listen completed')
    }
    this.loopbackServer = server
    this.loopbackPort = port
    return port
  }

  /**
   * Enable or disable the nearby listen. Rebinds when the address changes.
   *
   * @param enabled - whether nearby devices may connect.
   * @param bindAddress - IPv4 to bind; default is the preferred LAN literal.
   * @returns the nearby listen after the change.
   */
  async setNearby(
    enabled: boolean,
    bindAddress?: string,
  ): Promise<ShareStatus['nearby']> {
    if (!enabled) {
      await this.stopNearby()
      return null
    }
    const addresses = this.addressList()
    const bind = bindAddress ?? addresses[0]?.address
    if (bind === undefined) {
      throw new Error('share gateway: no LAN address for nearby listen')
    }
    if (!isAllowedNearbyBind(bind, addresses)) {
      throw new Error(`share gateway: bind address is not a listed LAN address: ${bind}`)
    }
    if (this.nearbyBind === bind && this.nearbyPort !== undefined) {
      return { bindAddress: bind, port: this.nearbyPort }
    }
    await this.stopNearby()
    const server = this.createListener('nearby')
    const port = await listen(server, bind)
    /* v8 ignore next 4 -- dispose racing nearby listen */
    if (this.disposed) {
      await closeServer(server, this.nearbySockets)
      throw new Error('share gateway: disposed before nearby listen completed')
    }
    this.nearbyServer = server
    this.nearbyBind = bind
    this.nearbyPort = port
    this.mintTicket('nearby', `${bind}:${String(port)}`)
    return { bindAddress: bind, port }
  }

  /**
   * Record the Tailscale Host the Serve hop will present, or clear it.
   *
   * @param audience - `machine.ts.net` or `machine.ts.net:8443`; null disables.
   */
  setTailscaleAudience(audience: string | null): void {
    if (audience === null || audience === '') {
      this.tailscaleAudience = undefined
      this.tailscaleGeneration += 1
      this.dropTickets('tailscale')
      this.dropSessions('tailscale')
      return
    }
    this.tailscaleAudience = audience
    this.mintTicket('tailscale', audience)
  }

  /**
   * Open a short window that pairs the next loopback GET of `/`.
   *
   * @param ttlMs - window length from the injectable clock.
   */
  openLoopbackPairing(ttlMs = LOOPBACK_PAIR_MS): void {
    this.loopbackPairUntil = this.now() + ttlMs
  }

  /**
   * Snapshot for the share window. Mints a ticket when a mode is on and none is live.
   *
   * @returns the current listen, audience, and pairing URLs.
   */
  status(): ShareStatus {
    this.refreshTickets()
    const nearby = this.nearbyBind !== undefined && this.nearbyPort !== undefined
      ? { bindAddress: this.nearbyBind, port: this.nearbyPort }
      : null
    return {
      loopbackPort: this.loopbackPort ?? null,
      nearby,
      tailscaleAudience: this.tailscaleAudience ?? null,
      addresses: this.addressList(),
      nearbyTicketUrl: nearby === null
        ? null
        : ticketUrl('http', `${nearby.bindAddress}:${String(nearby.port)}`, this.liveTicket('nearby')),
      tailscaleTicketUrl: this.tailscaleAudience === undefined
        ? null
        : ticketUrl('https', this.tailscaleAudience, this.liveTicket('tailscale')),
    }
  }

  /**
   * Close both listens and destroy tracked sockets.
   *
   * @returns a promise that settles after both servers close.
   */
  async dispose(): Promise<void> {
    this.disposed = true
    this.nearbyGeneration += 1
    this.tailscaleGeneration += 1
    this.sessions.clear()
    this.tickets.clear()
    const loopback = this.loopbackServer
    const nearby = this.nearbyServer
    this.loopbackServer = undefined
    this.nearbyServer = undefined
    this.loopbackPort = undefined
    this.nearbyBind = undefined
    this.nearbyPort = undefined
    this.tailscaleAudience = undefined
    await Promise.all([
      closeServer(loopback, this.loopbackSockets),
      closeServer(nearby, this.nearbySockets),
    ])
  }

  private addressList(): ShareAddress[] {
    return listShareAddresses(this.interfaces ?? networkInterfaces())
  }

  private async stopNearby(): Promise<void> {
    const server = this.nearbyServer
    this.nearbyServer = undefined
    this.nearbyBind = undefined
    this.nearbyPort = undefined
    this.nearbyGeneration += 1
    this.dropTickets('nearby')
    this.dropSessions('nearby')
    await closeServer(server, this.nearbySockets)
  }

  private createListener(kind: ListenerKind): Server {
    const sockets = kind === 'loopback' ? this.loopbackSockets : this.nearbySockets
    const server = createServer((req, res) => {
      void this.handleHttp(kind, req, res)
    })
    server.on('connection', (socket) => {
      sockets.add(socket)
      socket.on('close', () => { sockets.delete(socket) })
    })
    server.on('upgrade', (req, socket, head) => {
      sockets.add(socket)
      socket.on('close', () => { sockets.delete(socket) })
      this.handleUpgrade(kind, req, socket, head)
    })
    return server
  }

  private handleHttp(kind: ListenerKind, req: IncomingMessage, res: ServerResponse): Promise<void> {
    const pathname = requestPath(req)
    if (isDesktopControlPath(pathname)) {
      writeHtml(res, 404, PAGE_NOT_FOUND, req.method)
      return Promise.resolve()
    }
    if (isPairPath(pathname)) {
      this.handlePair(kind, req, res, pathname)
      return Promise.resolve()
    }
    const admitted = this.admit(kind, req, res)
    if (!admitted) return Promise.resolve()
    return proxyHttp(req, res, {
      sidecarPort: this.sidecarPort,
      launchToken: this.launchToken,
      upstreamHost: upstreamAuthority(admitted.kind, this.sidecarPort),
    })
  }

  private handleUpgrade(kind: ListenerKind, req: IncomingMessage, socket: Duplex, head: Buffer): void {
    const pathname = requestPath(req)
    if (isDesktopControlPath(pathname) || isPairPath(pathname)) {
      socket.write(refusalResponse(404))
      socket.destroy()
      return
    }
    const admitted = this.admitUpgrade(kind, req, socket)
    if (!admitted) return
    proxyUpgrade(req, socket, head, {
      sidecarPort: this.sidecarPort,
      launchToken: this.launchToken,
      upstreamHost: upstreamAuthority(admitted.kind, this.sidecarPort),
    })
  }

  private handlePair(
    kind: ListenerKind,
    req: IncomingMessage,
    res: ServerResponse,
    pathname: string,
  ): void {
    const ticketId = pathname.slice(PAIR_PREFIX.length + 1)
    if (req.method === 'GET' || req.method === 'HEAD') {
      writeHtml(res, 200, PAGE_INTERSTITIAL, req.method)
      return
    }
    if (req.method !== 'POST') {
      writeEmpty(res, 405, { allow: 'GET, HEAD, POST' })
      return
    }
    if (!browserMarkersOk(req)) {
      writeEmpty(res, 403)
      return
    }
    const ticket = this.tickets.get(ticketId)
    const host = headerValue(req.headers.host)
    if (
      ticket === undefined
      || ticket.consumed
      || ticket.expiresAt <= this.now()
      || host === undefined
      || !hostMatchesAudience(host, ticket.audienceHost)
      || !this.ticketListenerOk(kind, ticket.kind)
    ) {
      writeHtml(res, 410, PAGE_EXPIRED, req.method)
      return
    }
    ticket.consumed = true
    const session = this.createSession(ticket.kind)
    const secure = ticket.kind === 'tailscale'
    res.writeHead(303, {
      'cache-control': 'no-store',
      'referrer-policy': 'no-referrer',
      location: '/',
      'set-cookie': shareCookie(session.id, secure),
    })
    res.end()
  }

  private ticketListenerOk(listener: ListenerKind, kind: Ticket['kind']): boolean {
    if (kind === 'nearby') return listener === 'nearby'
    return listener === 'loopback'
  }

  private admit(
    kind: ListenerKind,
    req: IncomingMessage,
    res: ServerResponse,
  ): ShareSession | undefined {
    if (!browserMarkersOk(req)) {
      writeEmpty(res, 403)
      return undefined
    }
    const session = this.sessionFromCookie(req)
    if (session !== undefined && this.sessionAllowed(kind, session, req)) return session
    if (kind === 'loopback' && this.loopbackPairOpen(req)) {
      const minted = this.createSession('loopback')
      res.setHeader('set-cookie', shareCookie(minted.id, false))
      return minted
    }
    writeHtml(res, 401, PAGE_NEED_PAIR, req.method)
    return undefined
  }

  private admitUpgrade(
    kind: ListenerKind,
    req: IncomingMessage,
    socket: Duplex,
  ): ShareSession | undefined {
    if (!browserMarkersOk(req)) {
      socket.write(refusalResponse(403))
      socket.destroy()
      return undefined
    }
    const session = this.sessionFromCookie(req)
    if (session !== undefined && this.sessionAllowed(kind, session, req)) return session
    socket.write(refusalResponse(401))
    socket.destroy()
    return undefined
  }

  private sessionAllowed(kind: ListenerKind, session: ShareSession, req: IncomingMessage): boolean {
    if (session.kind === 'loopback') {
      return kind === 'loopback' && isLoopbackSocket(req) && isLoopbackHostHeader(headerValue(req.headers.host))
    }
    if (session.kind === 'nearby') {
      return kind === 'nearby'
        && session.generation === this.nearbyGeneration
        && this.nearbyHostOk(req)
    }
    return kind === 'loopback'
      && session.generation === this.tailscaleGeneration
      && this.tailscaleHostOk(req)
  }

  private loopbackPairOpen(req: IncomingMessage): boolean {
    return this.now() < this.loopbackPairUntil
      && isLoopbackSocket(req)
      && isLoopbackHostHeader(headerValue(req.headers.host))
  }

  private nearbyHostOk(req: IncomingMessage): boolean {
    /* v8 ignore next -- stopNearby drops nearby sessions before a request can observe a cleared bind */
    if (this.nearbyBind === undefined || this.nearbyPort === undefined) return false
    const host = headerValue(req.headers.host)
    if (host === undefined || !hostMatchesAudience(host, `${this.nearbyBind}:${String(this.nearbyPort)}`)) {
      return false
    }
    return canonicalIp(req.socket.localAddress) === this.nearbyBind
  }

  private tailscaleHostOk(req: IncomingMessage): boolean {
    const audience = this.tailscaleAudience
    const host = headerValue(req.headers.host)
    return audience !== undefined && host !== undefined && hostMatchesAudience(host, audience)
  }

  private sessionFromCookie(req: IncomingMessage): ShareSession | undefined {
    const id = cookieValue(headerValue(req.headers.cookie), SHARE_COOKIE)
    if (id === undefined) return undefined
    return this.sessions.get(id)
  }

  private createSession(kind: SessionKind): ShareSession {
    const id = randomBytes(16).toString('hex')
    const generation = kind === 'nearby'
      ? this.nearbyGeneration
      : kind === 'tailscale'
        ? this.tailscaleGeneration
        : 0
    const session: ShareSession = { id, kind, generation }
    this.sessions.set(id, session)
    return session
  }

  private mintTicket(kind: Ticket['kind'], audienceHost: string): Ticket {
    this.dropTickets(kind)
    const ticket: Ticket = {
      id: randomBytes(16).toString('hex'),
      kind,
      audienceHost,
      expiresAt: this.now() + TICKET_TTL_MS,
      consumed: false,
    }
    this.tickets.set(ticket.id, ticket)
    return ticket
  }

  private refreshTickets(): void {
    const nearby = this.nearbyBind !== undefined && this.nearbyPort !== undefined
      ? `${this.nearbyBind}:${String(this.nearbyPort)}`
      : undefined
    if (nearby !== undefined && this.liveTicket('nearby') === undefined) {
      this.mintTicket('nearby', nearby)
    }
    if (this.tailscaleAudience !== undefined && this.liveTicket('tailscale') === undefined) {
      this.mintTicket('tailscale', this.tailscaleAudience)
    }
  }

  private liveTicket(kind: Ticket['kind']): Ticket | undefined {
    const now = this.now()
    for (const ticket of this.tickets.values()) {
      if (ticket.kind === kind && !ticket.consumed && ticket.expiresAt > now) return ticket
    }
    return undefined
  }

  private dropTickets(kind: Ticket['kind']): void {
    for (const [id, ticket] of this.tickets) {
      if (ticket.kind === kind) this.tickets.delete(id)
    }
  }

  private dropSessions(kind: SessionKind): void {
    for (const [id, session] of this.sessions) {
      if (session.kind === kind) this.sessions.delete(id)
    }
  }
}

/**
 * POST `/__dshd_share`: header token plus loopback Host only. Cookie is ignored.
 *
 * @param req - incoming request.
 * @param res - response to write.
 * @param launchToken - per-launch token.
 * @param gateway - live gateway.
 * @returns a promise that settles after the response is written.
 */
export async function handleShareControl(
  req: IncomingMessage,
  res: ServerResponse,
  launchToken: string,
  gateway: ShareGateway,
): Promise<void> {
  if (req.method !== 'POST') {
    writeEmpty(res, 405, { allow: 'POST' })
    return
  }
  if (!isLoopbackHostHeader(headerValue(req.headers.host)) || !headerTokenMatches(req, launchToken)) {
    writeEmpty(res, 401)
    return
  }
  let body: string
  try {
    body = await readLimitedBody(req, CONTROL_BODY_LIMIT)
  } catch {
    writeEmpty(res, 413)
    return
  }
  let parsed: unknown
  try {
    parsed = body === '' ? { op: 'status' } : JSON.parse(body) as unknown
  } catch {
    writeEmpty(res, 400)
    return
  }
  if (typeof parsed !== 'object' || parsed === null || !('op' in parsed) || typeof parsed.op !== 'string') {
    writeEmpty(res, 400)
    return
  }
  try {
    await dispatchControl(parsed as ShareControlBody, gateway, res)
  } catch (error) {
    /* v8 ignore next -- setNearby throws Error; other ops do not throw */
    const message = error instanceof Error ? error.message : String(error)
    const payload = JSON.stringify({ error: message })
    res.writeHead(409, {
      'cache-control': 'no-store',
      'content-type': 'application/json',
      'content-length': Buffer.byteLength(payload),
    })
    res.end(payload)
  }
}

type ShareControlBody =
  | { op: 'status' }
  | { op: 'setNearby'; enabled: boolean; bindAddress?: string }
  | { op: 'setTailscaleAudience'; audience: string | null }
  | { op: 'openLoopback'; ttlMs?: number }

async function dispatchControl(
  body: ShareControlBody,
  gateway: ShareGateway,
  res: ServerResponse,
): Promise<void> {
  switch (body.op) {
    case 'status':
      break
    case 'setNearby':
      if (typeof body.enabled !== 'boolean') {
        writeEmpty(res, 400)
        return
      }
      await gateway.setNearby(body.enabled, body.bindAddress)
      break
    case 'setTailscaleAudience':
      if (body.audience !== null && typeof body.audience !== 'string') {
        writeEmpty(res, 400)
        return
      }
      gateway.setTailscaleAudience(body.audience)
      break
    case 'openLoopback':
      gateway.openLoopbackPairing(typeof body.ttlMs === 'number' ? body.ttlMs : LOOPBACK_PAIR_MS)
      break
    default:
      writeEmpty(res, 400)
      return
  }
  writeJson(res, gateway.status())
}

/**
 * Install the loopback gateway and the sidecar control route. No-op disposer
 * still closes the listen.
 *
 * @param ctx - plugin context carrying webServer.
 * @param session - live desktop bootstrap session.
 * @returns the fiber disposer.
 */
export function installShareGateway(ctx: Context, session: DesktopBootstrap): () => void {
  const gateway = new ShareGateway({
    launchToken: session.token,
    sidecarPort: ctx.webServer.port,
  })
  const started = gateway.listenLoopback()
  const disposeControl = ctx.webServer.register({
    kind: 'exact',
    path: SHARE_CONTROL_PATH,
    owner: 'web-app',
    handler: (req, res) => { void handleShareControl(req, res, session.token, gateway) },
  })
  return async () => {
    disposeControl()
    /* v8 ignore start -- loopback bind failure is a process-level condition; dispose still runs */
    try {
      await started
    } catch {
      // listenLoopback rejected because loopback bind failed; dispose still closes any server that bound.
    }
    /* v8 ignore stop */
    await gateway.dispose()
  }
}

interface ProxyTarget {
  sidecarPort: number
  launchToken: string
  upstreamHost: string
}

function proxyHttp(
  req: IncomingMessage,
  res: ServerResponse,
  target: ProxyTarget,
): Promise<void> {
  return new Promise((resolve) => {
    const headers = rewriteHeaders(req.headers, target, 'http')
    const proxy = httpRequest({
      hostname: SIDECAR_HOST,
      port: target.sidecarPort,
      /* v8 ignore next -- node:http always sets url on server requests */
      path: req.url ?? '/',
      method: req.method,
      headers,
    }, (upstream) => {
      const out = { ...omitHop(upstream.headers, 'http') }
      const fromUpstream = stripLaunchCookies(upstream.headers['set-cookie']) ?? []
      const already = res.getHeader('set-cookie')
      const fromAdmit = typeof already === 'string' ? [already] : []
      const cookies = [...fromAdmit, ...fromUpstream]
      if (cookies.length === 0) delete out['set-cookie']
      else out['set-cookie'] = cookies
      /* v8 ignore next -- node:http always sets statusCode on IncomingMessage */
      res.writeHead(upstream.statusCode ?? 502, out)
      upstream.pipe(res)
      upstream.on('end', () => { resolve() })
      /* v8 ignore next 4 -- upstream socket errors after headers */
      upstream.on('error', () => {
        res.destroy()
        resolve()
      })
    })
    proxy.on('error', () => {
      if (!res.headersSent) writeEmpty(res, 502)
      /* v8 ignore start -- headers already sent when the proxy fails mid-body */
      else res.destroy()
      /* v8 ignore stop */
      resolve()
    })
    req.pipe(proxy)
  })
}

function proxyUpgrade(
  req: IncomingMessage,
  socket: Duplex,
  head: Buffer,
  target: ProxyTarget,
): void {
  const headers = rewriteHeaders(req.headers, target, 'upgrade')
  const proxy = httpRequest({
    hostname: SIDECAR_HOST,
    port: target.sidecarPort,
    /* v8 ignore next -- node:http always sets url on server requests */
    path: req.url ?? '/',
    method: req.method,
    headers,
  })
  proxy.on('upgrade', (upstreamRes, upstreamSocket, upstreamHead) => {
    /* v8 ignore next -- node:http always sets statusCode and statusMessage on an upgrade */
    const lines = [`HTTP/1.1 ${String(upstreamRes.statusCode ?? 101)} ${upstreamRes.statusMessage ?? ''}`.trim()]
    appendHeaderLines(omitHop(upstreamRes.headers, 'upgrade'), lines)
    socket.write(`${lines.join('\r\n')}\r\n\r\n`)
    if (upstreamHead.length > 0) socket.write(upstreamHead)
    upstreamSocket.pipe(socket)
    socket.pipe(upstreamSocket)
  })
  proxy.on('response', (upstreamRes) => {
    /* v8 ignore next -- node:http always sets statusCode and statusMessage on a refused upgrade */
    const lines = [`HTTP/1.1 ${String(upstreamRes.statusCode ?? 502)} ${upstreamRes.statusMessage ?? ''}`]
    appendHeaderLines(omitHop(upstreamRes.headers, 'http'), lines)
    socket.write(`${lines.join('\r\n')}\r\n\r\n`)
    upstreamRes.pipe(socket)
  })
  proxy.on('error', () => { socket.destroy() })
  proxy.end(head)
}

function rewriteHeaders(
  headers: IncomingHttpHeaders,
  target: ProxyTarget,
  mode: 'http' | 'upgrade',
): IncomingHttpHeaders {
  const out = omitHop(headers, mode)
  out.host = target.upstreamHost
  out.origin = `http://${target.upstreamHost}`
  out['x-dsh-token'] = target.launchToken
  return out
}

function omitHop(headers: IncomingHttpHeaders, mode: 'http' | 'upgrade'): IncomingHttpHeaders {
  const out: IncomingHttpHeaders = {}
  for (const [name, value] of Object.entries(headers)) {
    const key = name.toLowerCase()
    if (STRIP_ALWAYS.has(key)) continue
    if (mode === 'http' && STRIP_HTTP.has(key)) continue
    out[key] = value
  }
  return out
}

function appendHeaderLines(headers: IncomingHttpHeaders, lines: string[]): void {
  for (const [name, value] of Object.entries(headers)) {
    if (Array.isArray(value)) {
      for (const item of value) lines.push(`${name}: ${item}`)
      continue
    }
    /* v8 ignore next -- Node omits undefined header values from IncomingMessage */
    if (value === undefined) continue
    lines.push(`${name}: ${value}`)
  }
}

function stripLaunchCookies(values: string | string[] | undefined): string[] | undefined {
  if (values === undefined) return undefined
  const list = [values].flat()
  const kept = list.filter(item => !item.toLowerCase().startsWith('dsh-token='))
  return kept.length === 0 ? undefined : kept
}

function upstreamAuthority(kind: SessionKind, sidecarPort: number): string {
  const host = kind === 'loopback' ? SIDECAR_HOST : SHARE_INTERNAL_HOST
  return `${host}:${String(sidecarPort)}`
}

function ticketUrl(scheme: 'http' | 'https', audienceHost: string, ticket: Ticket | undefined): string | null {
  /* v8 ignore next -- status() remints before reading a live ticket */
  if (ticket === undefined) return null
  return `${scheme}://${audienceHost}${PAIR_PREFIX}/${ticket.id}`
}

function shareCookie(value: string, secure: boolean): string {
  return `${SHARE_COOKIE}=${encodeURIComponent(value)}; Path=/; HttpOnly; SameSite=Strict${secure ? '; Secure' : ''}`
}

function isPairPath(pathname: string): boolean {
  return pathname === PAIR_PREFIX || pathname.startsWith(`${PAIR_PREFIX}/`)
}

function isDesktopControlPath(pathname: string): boolean {
  return pathname === '/__dshd' || pathname.startsWith('/__dshd_') || pathname.startsWith('/__dshd/')
}

function requestPath(req: IncomingMessage): string {
  /* v8 ignore next -- node:http always sets url on server requests */
  return new URL(req.url ?? '/', 'http://x').pathname
}

function browserMarkersOk(req: IncomingMessage): boolean {
  if (headerValue(req.headers['sec-fetch-site']) === 'cross-site') return false
  const origin = headerValue(req.headers.origin)
  const host = headerValue(req.headers.host)
  if (origin === undefined) return true
  /* v8 ignore next -- Origin is only sent with a Host on HTTP/1.1 */
  if (host === undefined) return false
  try {
    return new URL(origin).host === new URL(`http://${host}`).host
  } catch {
    return false
  }
}

function headerValue(value: string | string[] | undefined): string | undefined {
  if (typeof value === 'string' && value !== '') return value
  /* v8 ignore next -- Host/Origin/Cookie are strings on node:http IncomingMessage */
  return undefined
}

function headerTokenMatches(req: IncomingMessage, expected: string): boolean {
  const presented = headerValue(req.headers['x-dsh-token'])
  if (presented === undefined) return false
  const left = Buffer.from(presented)
  const right = Buffer.from(expected)
  if (left.length !== right.length) return false
  return timingSafeEqual(left, right)
}

function cookieValue(cookieHeader: string | undefined, name: string): string | undefined {
  if (cookieHeader === undefined) return undefined
  for (const part of cookieHeader.split(';')) {
    const trimmed = part.trim()
    const eq = trimmed.indexOf('=')
    if (eq <= 0) continue
    if (trimmed.slice(0, eq) !== name) continue
    try {
      return decodeURIComponent(trimmed.slice(eq + 1))
    } catch {
      return undefined
    }
  }
  return undefined
}

function isLoopbackHostHeader(host: string | undefined): boolean {
  /* v8 ignore next -- HTTP/1.1 requests always carry Host */
  if (host === undefined) return false
  if (host.startsWith('[::1]') || /^::1:\d+$/.test(host) || host === '::1') return true
  try {
    const hostname = new URL(`http://${host}`).hostname
    return hostname === 'localhost' || isLoopbackIpv4(hostname)
  } catch {
    return false
  }
}

/**
 * Whether nearby may listen on `bind`. Listed LAN IPv4s are allowed; loopback
 * is allowed so tests can bind without a real NIC. `0.0.0.0` is never listed
 * and is not loopback.
 *
 * @param bind - requested IPv4.
 * @param listed - addresses from {@link listShareAddresses}.
 * @returns whether the gateway may listen on `bind`.
 */
export function isAllowedNearbyBind(bind: string, listed: ShareAddress[]): boolean {
  if (listed.some(entry => entry.address === bind)) return true
  return isLoopbackIpv4(bind)
}

function isLoopbackIpv4(hostname: string): boolean {
  const parts = hostname.split('.')
  return parts.length === 4
    && parts[0] === '127'
    && parts.every(part => /^\d{1,3}$/.test(part) && Number(part) <= 255)
}

function isLoopbackSocket(req: IncomingMessage): boolean {
  return isLoopbackIp(canonicalIp(req.socket.remoteAddress))
}

function canonicalIp(address: string | undefined): string {
  /* v8 ignore next -- node:http sockets always have a remoteAddress after accept */
  if (address === undefined) return ''
  return address.replace(/^::ffff:/i, '')
}

function isLoopbackIp(address: string): boolean {
  return address === '::1' || isLoopbackIpv4(address)
}

function hostMatchesAudience(hostHeader: string, audience: string): boolean {
  try {
    const presented = new URL(`http://${hostHeader}`)
    const expected = new URL(`http://${audience}`)
    if (presented.hostname.toLowerCase() !== expected.hostname.toLowerCase()) return false
    if (expected.port === '') return true
    const presentedPort = presented.port === '' ? '80' : presented.port
    return presentedPort === expected.port
  } catch {
    return false
  }
}

function shareAddressScore(item: ShareAddress): number {
  if (item.address.startsWith('192.168.')) return 3
  if (item.address.startsWith('10.')) return 2
  if (isRfc1918(item.address)) return 1
  return 0
}

function isRfc1918(address: string): boolean {
  const parts = address.split('.').map(Number)
  return parts[0] === 172 && parts[1] !== undefined && parts[1] >= 16 && parts[1] <= 31
}

function isDockerBridge(address: string): boolean {
  const parts = address.split('.').map(Number)
  return parts[0] === 172 && parts[1] === 17
}

function isLinkLocal(address: string): boolean {
  const parts = address.split('.').map(Number)
  return parts[0] === 169 && parts[1] === 254
}

async function listen(server: Server, host: string): Promise<number> {
  return await new Promise((resolve, reject) => {
    const onError = (error: Error): void => { reject(error) }
    server.once('error', onError)
    server.listen(0, host, () => {
      server.off('error', onError)
      const addr = server.address()
      /* v8 ignore next 4 -- node:http listen on a TCP host always yields AddressInfo */
      if (addr === null || typeof addr === 'string') {
        reject(new Error('share gateway: expected tcp address'))
        return
      }
      resolve(addr.port)
    })
  })
}

async function closeServer(server: Server | undefined, sockets: Set<Duplex>): Promise<void> {
  if (server === undefined) return
  const closed = new Promise<void>((resolve) => {
    server.close(() => { resolve() })
  })
  server.closeAllConnections()
  for (const socket of sockets) socket.destroy()
  sockets.clear()
  await closed
}

function writeHtml(res: ServerResponse, status: number, body: string, method?: string): void {
  res.writeHead(status, {
    'cache-control': 'no-store',
    'referrer-policy': 'no-referrer',
    'content-type': 'text/html; charset=utf-8',
    'content-length': Buffer.byteLength(body),
  })
  if (method === 'HEAD') {
    res.end()
    return
  }
  res.end(body)
}

function writeEmpty(
  res: ServerResponse,
  status: number,
  extra: Record<string, string> = {},
): void {
  res.writeHead(status, { 'cache-control': 'no-store', ...extra })
  res.end()
}

function writeJson(res: ServerResponse, value: unknown): void {
  const body = JSON.stringify(value)
  res.writeHead(200, {
    'cache-control': 'no-store',
    'content-type': 'application/json',
    'content-length': Buffer.byteLength(body),
  })
  res.end(body)
}

function readLimitedBody(req: IncomingMessage, limit: number): Promise<string> {
  return new Promise((resolve, reject) => {
    const chunks: Buffer[] = []
    let size = 0
    let settled = false
    req.on('data', (chunk: Buffer | string) => {
      /* v8 ignore next -- further chunks after the size limit is exceeded */
      if (settled) return
      const buf = Buffer.from(chunk)
      size += buf.length
      if (size > limit) {
        settled = true
        reject(new Error('body too large'))
        return
      }
      chunks.push(buf)
    })
    req.on('end', () => {
      if (settled) return
      settled = true
      resolve(Buffer.concat(chunks).toString('utf8'))
    })
    /* v8 ignore next 6 -- IncomingMessage error after the request is already settled */
    req.on('error', (error) => {
      if (settled) return
      settled = true
      reject(error)
    })
  })
}

function refusalResponse(status: number): string {
  return [
    `HTTP/1.1 ${String(status)}`,
    'Connection: close',
    'Content-Length: 0',
    '',
    '',
  ].join('\r\n')
}

const PAGE_INTERSTITIAL = '<!doctype html><html lang="zh-CN"><head><meta charset="utf-8"/><meta name="viewport" content="width=device-width,initial-scale=1"/><meta name="referrer" content="no-referrer"/><title>打开 dshd</title></head><body><p>用手机相机扫码后会打开这台电脑上的 dshd。微信请选在 Safari 中打开。</p><form method="post" action=""><button type="submit">打开</button></form></body></html>'

const PAGE_EXPIRED = '<!doctype html><html lang="zh-CN"><head><meta charset="utf-8"/><title>码已失效</title></head><body><p>此码已失效。请回到电脑上重新扫码。</p></body></html>'

const PAGE_NEED_PAIR = '<!doctype html><html lang="zh-CN"><head><meta charset="utf-8"/><title>请再点一次</title></head><body><p>请回到 Desktop 再点一次「在浏览器中打开」，或重新扫码。</p></body></html>'

const PAGE_NOT_FOUND = '<!doctype html><html lang="zh-CN"><head><meta charset="utf-8"/><title>未找到</title></head><body><p>未找到。</p></body></html>'
