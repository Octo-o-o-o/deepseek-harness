/**
 * @deepseek-ai/dsh-host-webserver — Web route-registration plugin: a node:http
 * server plus the `webServer` service (HTTP and upgrade route registries, the
 * single admission-guard seat every request passes before routing, index
 * transform taps, and the single fallback seat for everything no route
 * claims). Knows no harness concepts and serves no files; the composing
 * application's frontend plugin owns dist serving through the fallback hook.
 * Web shape only — Electron loads dist over file:// and carries fetch over an
 * IPC bridge. This package never prints: the URL line belongs to the shell.
 */

import { createServer, STATUS_CODES } from 'node:http'
import type { IncomingMessage, ServerResponse, Server } from 'node:http'
import type { AddressInfo } from 'node:net'
import type { Duplex } from 'node:stream'
import { Context, Service } from '@deepseek-ai/cordis'
import z from '@deepseek-ai/schemastery'

declare module '@deepseek-ai/cordis' {
  interface Context {
    webServer: WebServer
  }
}

/** Route match kind: 'exact' matches the pathname verbatim; 'prefix' p matches p and p/<anything>. */
export type WebRouteKind = 'exact' | 'prefix'

/** Kind recorded by {@link WebServer.listRegistrations}. */
export type WebRegistrationKind = WebRouteKind | 'upgrade' | 'fallback'

/** One live registration for composition audit. */
export interface WebRegistration {
  kind: WebRegistrationKind
  /** Absolute pathname, or `*` for the fallback seat. */
  path: string
  /** Plugin id that claimed the route; `unknown` when the caller omitted it. */
  owner: string
}

/** One named route registration. */
export interface WebRoute {
  kind: WebRouteKind
  /** Absolute pathname, no trailing slash. */
  path: string
  /** Owns the full response lifecycle (may hold the response open, e.g. SSE). */
  handler: (req: IncomingMessage, res: ServerResponse) => void | Promise<void>
  /** Registration owner for desktop `/api` audit. */
  owner?: string
}

/**
 * Admission check the server runs on every HTTP request and every upgrade
 * before it consults any route table. Route handlers can only fence the routes
 * they own, so authorization that must hold for the whole server — including
 * paths registered by plugins the deployment does not control — belongs here.
 *
 * @param req - the incoming request; headers carry whatever the check reads.
 * @param pathname - the request pathname, parsed once by the server.
 * @returns undefined to admit the request, or the HTTP status refusing it.
 */
export type WebRequestGuard = (req: IncomingMessage, pathname: string) => number | undefined

/** One exact-path HTTP upgrade registration. */
export interface WebUpgradeRoute {
  /** Absolute pathname, no trailing slash. */
  path: string
  /** Owns protocol negotiation and the upgraded socket after dispatch. */
  handler: (req: IncomingMessage, socket: Duplex, head: Buffer) => void | Promise<void>
  /** Registration owner for desktop `/api` audit. */
  owner?: string
}

/** Gateway config: the listen address. */
export interface Config {
  /** Listen host; the two supported values are loopback and all-interfaces. */
  host: '127.0.0.1' | '0.0.0.0'
  /** Listen port; zero requests an OS-assigned port. */
  port: number
}

/**
 * Status-only HTTP response refusing one upgrade on the raw socket, which
 * carries no ServerResponse to write through.
 */
function refusalResponse(status: number): string {
  return [
    `HTTP/1.1 ${String(status)} ${STATUS_CODES[status] ?? ''}`,
    'Connection: close',
    'Content-Length: 0',
    '',
    '',
  ].join('\r\n')
}

/**
 * The browser HTTP carrier service. Activation listens immediately. Route
 * registration order does not affect requests because configured named routes
 * must be distinct, and the fallback handler answers anything not yet claimed
 * during startup with 404 until its owner registers. A listen failure rejects
 * initialization, and the boot process reports the failed fiber.
 */
export class WebServer extends Service {
  static Config: z<Config> = z.object({
    host: z.union([z.const('127.0.0.1'), z.const('0.0.0.0')]).required(),
    port: z.natural().max(65535).required(),
  })

  private readonly exact = new Map<string, WebRoute>()
  private readonly prefixes = new Map<string, WebRoute>()
  private readonly upgrades = new Map<string, WebUpgradeRoute>()
  private readonly upgradedSockets = new Set<Duplex>()
  private readonly indexTaps: ((html: string) => string)[] = []
  private readonly registrations: WebRegistration[] = []
  private guard: WebRequestGuard | undefined
  private fallback: WebRoute['handler'] | undefined
  private server!: Server
  private listenedPort!: number

  constructor(ctx: Context, private config: Config) {
    super(ctx, 'webServer')
  }

  /** The listening port (the OS-assigned value when config.port is 0). */
  get port(): number {
    return this.listenedPort
  }

  /** The configured bind host (the loopback or all-interfaces literal). */
  get host(): Config['host'] {
    return this.config.host
  }

  /**
   * Claim the admission-guard seat: the {@link WebRequestGuard} every HTTP
   * request and upgrade passes before route matching, so an authorization rule
   * covers the whole server instead of only the routes whose owners implement
   * it. One owner only — a second registration throws, because a request
   * either is admitted or is not, and two seats could not both decide that.
   * The seat is optional: a composition that leaves it empty routes every
   * request, which is the posture of a deployment whose routes fence
   * themselves. Guards claim no route, so they do not appear in
   * {@link WebServer.listRegistrations}.
   * @param guard - returns undefined to admit, or the refusing HTTP status.
   * @returns the disposer releasing the seat.
   */
  registerGuard(guard: WebRequestGuard): () => void {
    if (this.guard !== undefined) {
      throw new Error('webserver: guard already registered')
    }
    this.guard = guard
    return () => {
      this.guard = undefined
    }
  }

  /**
   * Register a named route. Duplicate (kind, path) throws — route patterns are
   * a composition-level contract, so a collision is a misconfiguration.
   * @param route - kind, path, and the owning handler.
   * @returns the disposer removing the route.
   */
  register(route: WebRoute): () => void {
    const table = route.kind === 'exact' ? this.exact : this.prefixes
    if (table.has(route.path)) {
      throw new Error(`webserver: duplicate ${route.kind} route "${route.path}"`)
    }
    table.set(route.path, route)
    const forget = this.record(route.kind, route.path, route.owner)
    return () => {
      table.delete(route.path)
      forget()
    }
  }

  /**
   * Register an exact-path HTTP upgrade route. Duplicate paths throw because
   * one socket can have only one protocol owner.
   * @param route - pathname and handler owning negotiation plus socket use.
   * @returns the disposer removing the route.
   */
  registerUpgrade(route: WebUpgradeRoute): () => void {
    if (this.upgrades.has(route.path)) {
      throw new Error(`webserver: duplicate upgrade route "${route.path}"`)
    }
    this.upgrades.set(route.path, route)
    const forget = this.record('upgrade', route.path, route.owner)
    return () => {
      this.upgrades.delete(route.path)
      forget()
    }
  }

  /**
   * Claim the fallback seat: the handler answering every request no named
   * route matches (the SPA dist server in the shipped Web composition). One
   * owner only — a second registration throws, because two fallbacks cannot
   * compose.
   * @param handler - owns the full response lifecycle of unmatched requests.
   * @param owner - registration owner for composition audit.
   * @returns the disposer releasing the seat.
   */
  registerFallback(handler: WebRoute['handler'], owner: string = 'unknown'): () => void {
    if (this.fallback !== undefined) {
      throw new Error('webserver: fallback already registered')
    }
    this.fallback = handler
    const forget = this.record('fallback', '*', owner)
    return () => {
      this.fallback = undefined
      forget()
    }
  }

  /**
   * Snapshot of named HTTP, upgrade, and fallback registrations.
   *
   * @returns a copy of each live registration's kind, path, and owner.
   */
  listRegistrations(): WebRegistration[] {
    return this.registrations.slice()
  }

  private record(kind: WebRegistrationKind, path: string, owner: string | undefined): () => void {
    const entry: WebRegistration = { kind, path, owner: owner ?? 'unknown' }
    this.registrations.push(entry)
    return () => {
      const at = this.registrations.indexOf(entry)
      if (at !== -1) this.registrations.splice(at, 1)
    }
  }

  /**
   * Register an index.html transform, applied by the fallback owner to every
   * index response ({@link applyIndexTaps}) in registration order.
   * @param transform - pure html-to-html function.
   * @returns the disposer removing the transform.
   */
  tapIndex(transform: (html: string) => string): () => void {
    this.indexTaps.push(transform)
    return () => {
      const at = this.indexTaps.indexOf(transform)
      if (at !== -1) this.indexTaps.splice(at, 1)
    }
  }

  /** Listen; resolves once the socket is bound (rejection = FAILED fiber). */
  async [Service.init](): Promise<void> {
    const handle = async (req: IncomingMessage, res: ServerResponse): Promise<void> => {
      /* v8 ignore next -- `?? '/'` arm: node:http always sets url on server
      requests; the field is only optional on the client-side IncomingMessage type */
      const rawPath = new URL(req.url ?? '/', 'http://x').pathname
      const refused = this.guard?.(req, rawPath)
      if (refused !== undefined) {
        res.writeHead(refused)
        res.end()
        return
      }
      const route = this.match(rawPath)
      if (route !== undefined) {
        await route.handler(req, res)
        return
      }
      const fallback = this.fallback
      if (fallback === undefined) {
        res.writeHead(404)
        res.end()
        return
      }
      await fallback(req, res)
    }
    // Last-resort guard: handle() rejecting would otherwise be an unhandled
    // rejection killing the process on one malformed request (bad %-escape,
    // client dropping mid-body). Per-request failures log and answer 400 —
    // never a process exit.
    this.server = createServer((req, res) => {
      handle(req, res).catch((err: unknown) => {
        this.ctx.logger.warn(err instanceof Error ? err : new Error(String(err)))
        if (res.headersSent) {
          res.destroy()
          return
        }
        res.writeHead(400)
        res.end()
      })
    })
    this.server.on('upgrade', (req, socket, head) => {
      const onError = (error: Error): void => {
        this.ctx.logger.warn(error)
        socket.destroy()
      }
      socket.on('error', onError)
      socket.once('close', () => {
        socket.off('error', onError)
        this.upgradedSockets.delete(socket)
      })
      let route: WebUpgradeRoute | undefined
      try {
        /* v8 ignore next -- node:http always sets url on server requests. */
        const pathname = new URL(req.url ?? '/', 'http://x').pathname
        const refused = this.guard?.(req, pathname)
        if (refused !== undefined) {
          // Written before any handshake, so the client sees the refusal
          // rather than a bare reset it could only read as a transport fault.
          socket.end(refusalResponse(refused))
          return
        }
        route = this.upgrades.get(pathname)
      } catch (error) {
        this.ctx.logger.warn(error instanceof Error ? error : new Error(String(error)))
        socket.destroy()
        return
      }
      if (route === undefined) {
        socket.destroy()
        return
      }
      this.upgradedSockets.add(socket)
      try {
        Promise.resolve(route.handler(req, socket, head)).catch((error: unknown) => {
          this.ctx.logger.warn(error instanceof Error ? error : new Error(String(error)))
          socket.destroy()
        })
      } catch (error) {
        this.ctx.logger.warn(error instanceof Error ? error : new Error(String(error)))
        socket.destroy()
      }
    })

    await new Promise<void>((resolve, reject) => {
      this.server.once('error', reject)
      this.server.listen(this.config.port, this.config.host, () => {
        this.server.off('error', reject)
        this.server.on('error', (err) => { this.ctx.logger.error(err) })
        this.listenedPort = (this.server.address() as AddressInfo).port
        resolve()
      })
    })

    // Node does not include upgraded sockets in closeAllConnections(). The service
    // owns them with the other connections, so it tracks and destroys them explicitly.
    this.ctx.effect(() => async () => {
      const serverClosed = new Promise<void>((resolve) => {
        this.server.close(() => { resolve() })
      })
      this.server.closeAllConnections()
      const upgradedClosed = [...this.upgradedSockets].map(socket => new Promise<void>((resolve) => {
        socket.once('close', () => { resolve() })
        socket.destroy()
      }))
      await Promise.all([serverClosed, ...upgradedClosed])
    }, 'webServer.listen')
  }

  /** Longest-prefix-wins over the prefix table after an exact-table miss. */
  private match(pathname: string): WebRoute | undefined {
    const exact = this.exact.get(pathname)
    if (exact !== undefined) return exact
    let best: WebRoute | undefined
    for (const [prefix, route] of this.prefixes) {
      if (pathname !== prefix && !pathname.startsWith(`${prefix}/`)) continue
      if (best === undefined || prefix.length > best.path.length) best = route
    }
    return best
  }

  /**
   * Run an index.html body through the registered taps in registration order
   * — called by the fallback owner on every index response it renders.
   * @param html - the raw index.html body.
   * @returns the transformed body.
   */
  applyIndexTaps(html: string): string {
    let out = html
    for (const transform of this.indexTaps) out = transform(out)
    return out
  }
}

export default WebServer
