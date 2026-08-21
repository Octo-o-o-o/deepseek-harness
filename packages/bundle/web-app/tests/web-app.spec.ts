/**
 * Web runtime glue behavior: dist resolution through the bundle's own hook,
 * the frontend-static child claiming the fallback seat, the web-surface
 * prompt section and bash runtime variables, and readiness publication through
 * the URL line and default-browser handoff.
 */

import { EventEmitter } from 'node:events'
import { spawn, type ChildProcess } from 'node:child_process'
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { createServer } from 'node:http'
import type { IncomingMessage } from 'node:http'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { PassThrough } from 'node:stream'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { Context } from '@deepseek-ai/cordis'
import { createLaunchEnvironmentSnapshot, DSH_LAUNCH_ENVIRONMENT_KEY } from '@deepseek-ai/dsh-launch-environment'
import SystemPrompt from '@deepseek-ai/dsh-system-prompt'
import type { WebRequestGuard, WebServer } from '@deepseek-ai/dsh-host-webserver'
import { apply, Config, internals } from '../src/index.ts'
import { injectDesktopBootstrapScript } from '../src/desktop-bootstrap.ts'
import { SHARE_INTERNAL_HOST } from '../src/share-gateway.ts'
import { WEB_STARTUP_SERVICE } from '../src/startup.ts'

vi.mock('node:child_process', async importOriginal => ({
  ...await importOriginal<typeof import('node:child_process')>(),
  spawn: vi.fn(),
}))

vi.mock('node:os', async importOriginal => ({
  ...await importOriginal<typeof import('node:os')>(),
  networkInterfaces: () => ({
    lo0: [{ family: 'IPv4', internal: true, address: '127.0.0.1' }],
    en0: [{ family: 'IPv4', internal: false, address: '192.168.1.5' }],
  }),
}))

let dist: string | undefined

beforeEach(() => {
  vi.stubEnv('SSH_CONNECTION', '')
  vi.stubEnv('SSH_TTY', '')
})

afterEach(() => {
  vi.restoreAllMocks()
  vi.mocked(spawn).mockReset()
  vi.unstubAllEnvs()
  internals.resolveDistIndex = originalResolve
  internals.openBrowser = originalOpenBrowser
  if (dist !== undefined) rmSync(dist, { recursive: true, force: true })
  dist = undefined
})

const originalResolve = internals.resolveDistIndex
const originalOpenBrowser = internals.openBrowser

type BrowserLauncher = ChildProcess & { stderr: PassThrough }

/** Minimal browser-launcher process for the native handoff adapter. */
function launcher(): BrowserLauncher {
  return Object.assign(new EventEmitter(), { stderr: new PassThrough() }) as unknown as BrowserLauncher
}

/** Stage a dist fixture and point the bundle's resolver at it. */
function stageDist(): string {
  dist = mkdtempSync(join(tmpdir(), 'dsh-web-app-'))
  mkdirSync(join(dist, 'dist'))
  const index = join(dist, 'dist', 'index.html')
  writeFileSync(index, '<head></head><body>shell</body>')
  internals.resolveDistIndex = () => index
  return index
}

/** A fake webServer capturing the fallback seat, the guard seat, and index taps. */
function fakeHttpServer(
  host: '127.0.0.1' | '0.0.0.0' = '127.0.0.1',
  registrations: { kind: string; path: string; owner: string }[] = [
    { kind: 'prefix', path: '/api', owner: 'client-connection' },
  ],
): {
  server: WebServer
  seat: () => unknown
  guard: () => WebRequestGuard | undefined
  applyTaps: (html: string) => string
  handlers: Map<string, (req: IncomingMessage, res: import('node:http').ServerResponse) => void>
} {
  let fallback: unknown
  let guard: WebRequestGuard | undefined
  const taps: ((html: string) => string)[] = []
  const handlers = new Map<string, (req: IncomingMessage, res: import('node:http').ServerResponse) => void>()
  const server = {
    host,
    port: 4567,
    register: (route: { path: string; handler: (req: IncomingMessage, res: import('node:http').ServerResponse) => void }) => {
      handlers.set(route.path, route.handler)
      return () => { handlers.delete(route.path) }
    },
    registerGuard: (installed: WebRequestGuard) => {
      guard = installed
      return () => { guard = undefined }
    },
    listRegistrations: () => registrations,
    registerFallback: (handler: unknown) => {
      fallback = handler
      return () => { fallback = undefined }
    },
    tapIndex: (transform: (html: string) => string) => {
      taps.push(transform)
      return () => {
        const at = taps.indexOf(transform)
        if (at !== -1) taps.splice(at, 1)
      }
    },
    applyIndexTaps: (html: string) => taps.reduce((acc, tap) => tap(acc), html),
    renderIndex: (html: string) => taps.reduce((acc, tap) => tap(acc), html),
  } as unknown as WebServer
  return {
    server,
    seat: () => fallback,
    guard: () => guard,
    applyTaps: html => taps.reduce((acc, tap) => tap(acc), html),
    handlers,
  }
}

/** Minimal request stand-in: the desktop guard reads only headers. */
function requestWith(headers: Record<string, string>): IncomingMessage {
  return { headers } as unknown as IncomingMessage
}

/** A fake Loader whose settlement the test controls (the URL line waits on it). */
function provideLoader(ctx: Context, settle: () => Promise<void> = async () => {}): void {
  ctx.provide('loader', { await: settle } as never)
}

interface BashContribution {
  name: string
  variables: Record<string, { description: string }>
  resolve: () => Record<string, string>
}

describe('web-app runtime glue', () => {
  it('mounts dist serving, prompt section, bash variables, and publishes the URL with the LAN snapshot', async () => {
    stageDist()
    const ctx = new Context()
    // Editor markers and a project .env SSH value do not establish a remote launch.
    ctx.provide(DSH_LAUNCH_ENVIRONMENT_KEY, createLaunchEnvironmentSnapshot([
      { source: 'process', values: { VSCODE_IPC_HOOK_CLI: '/tmp/local-vscode-ipc' } },
      { source: 'project-env', path: '/work/.env', values: { SSH_CONNECTION: 'stale-project-value' } },
    ]))
    const { server, seat } = fakeHttpServer('0.0.0.0')
    ctx.provide('webServer', server)
    const contributions: BashContribution[] = []
    ctx.provide('shellEnv', {
      register: (contribution: BashContribution) => {
        contributions.push(contribution)
        return () => {}
      },
    } as never)
    provideLoader(ctx)
    const lifecycle: string[] = []
    const log = vi.spyOn(console, 'log').mockImplementation((message) => { lifecycle.push(String(message)) })
    const openBrowser = vi.fn(async (url: string) => { lifecycle.push(`open:${url}`) })
    internals.openBrowser = openBrowser
    apply(ctx, new Config({ openBrowser: true, printUrl: true, surfaceContext: true, trustedHosts: ['lab.internal'] }))
    await ctx.plugin(SystemPrompt, { persona: '' })
    // Settle the injected registrations.
    await new Promise(resolve => setTimeout(resolve, 0))

    expect(seat()).toBeDefined() // frontend-static claimed the fallback
    expect(ctx.get('webRuntime')).toEqual({
      lanAddresses: ['192.168.1.5'],
      trustedHosts: ['192.168.1.5', 'lab.internal'],
    })
    expect(log).toHaveBeenCalledWith('dsh web: http://127.0.0.1:4567 (LAN: http://192.168.1.5:4567)')
    expect(log).toHaveBeenCalledWith('dsh web: opening the default browser; pass --no-open to disable')
    expect(openBrowser).toHaveBeenCalledWith('http://127.0.0.1:4567')
    expect(lifecycle).toEqual([
      'dsh web: http://127.0.0.1:4567 (LAN: http://192.168.1.5:4567)',
      'dsh web: opening the default browser; pass --no-open to disable',
      'open:http://127.0.0.1:4567',
    ])
    const assembly = await ctx.systemPrompt.assemble()
    expect(assembly.sections.find(entry => entry.name === 'harness:source')?.text).toContain('DeepSeek Harness implementation checkout')
    const section = assembly.sections.find(entry => entry.name === 'app:web-surface')
    expect(section?.text).toContain('http://127.0.0.1:4567')
    // The single update contract: the receiver is always on; no-refresh
    // reloads additionally need the rebuild watcher.
    expect(section?.text).toContain('pnpm run dev:web')
    const webRuntime = contributions.find(contribution => contribution.name === 'web-runtime')
    expect(webRuntime?.resolve()).toEqual({ DSH_WEB_URL: 'http://127.0.0.1:4567' })
    await ctx.fiber.dispose()
  })

  it('publishes no readiness side effect when printing and browser opening are disabled', async () => {
    stageDist()
    const ctx = new Context()
    ctx.provide('webServer', fakeHttpServer().server)
    const log = vi.spyOn(console, 'log').mockImplementation(() => {})
    const openBrowser = vi.fn(async () => {})
    internals.openBrowser = openBrowser
    apply(ctx, new Config({ openBrowser: false, printUrl: false, surfaceContext: true, trustedHosts: [] }))
    await ctx.plugin(SystemPrompt, { persona: '' })
    await new Promise(resolve => setTimeout(resolve, 0))
    expect(log).not.toHaveBeenCalled()
    expect(openBrowser).not.toHaveBeenCalled()
    const assembly = await ctx.systemPrompt.assemble()
    expect(assembly.sections.find(entry => entry.name === 'app:web-surface')?.text)
      .toContain('rebuilding the affected Web artifacts')
    await ctx.fiber.dispose()
  })

  it('skips the surface context when disabled (the one-shot layer): no prompt section, no bash variables', async () => {
    stageDist()
    const ctx = new Context()
    ctx.provide('webServer', fakeHttpServer().server)
    const contributions: BashContribution[] = []
    ctx.provide('shellEnv', {
      register: (contribution: BashContribution) => {
        contributions.push(contribution)
        return () => {}
      },
    } as never)
    apply(ctx, new Config({ openBrowser: false, printUrl: false, surfaceContext: false, trustedHosts: [] }))
    await ctx.plugin(SystemPrompt, { persona: '' })
    await new Promise(resolve => setTimeout(resolve, 0))
    const assembly = await ctx.systemPrompt.assemble()
    expect(assembly.sections.some(entry => entry.name === 'app:web-surface')).toBe(false)
    expect(assembly.sections.some(entry => entry.name === 'harness:source')).toBe(false)
    expect(contributions).toEqual([])
    await ctx.fiber.dispose()
  })

  it('prints the loopback-only URL line when no LAN snapshot exists', async () => {
    stageDist()
    const ctx = new Context()
    ctx.provide('webServer', fakeHttpServer().server)
    const log = vi.spyOn(console, 'log').mockImplementation(() => {})
    apply(ctx, new Config({ openBrowser: false, printUrl: true, surfaceContext: true, trustedHosts: [] }))
    await new Promise(resolve => setTimeout(resolve, 0))
    expect(log).toHaveBeenCalledWith('dsh web: http://127.0.0.1:4567')
    await ctx.fiber.dispose()
  })

  it.each([
    ['SSH_CONNECTION', '10.0.0.2 55000 10.0.0.9 22'],
    ['SSH_TTY', '/dev/pts/3'],
  ] as const)('prints the host URL but skips browser handoff when %s marks an SSH launch', async (name, value) => {
    vi.stubEnv(name, value)
    stageDist()
    const ctx = new Context()
    ctx.provide('webServer', fakeHttpServer().server)
    const log = vi.spyOn(console, 'log').mockImplementation(() => {})
    const openBrowser = vi.fn(async () => {})
    internals.openBrowser = openBrowser
    apply(ctx, new Config({ openBrowser: true, printUrl: true, surfaceContext: false, trustedHosts: [] }))
    await new Promise(resolve => setTimeout(resolve, 0))
    expect(log).toHaveBeenCalledWith('dsh web: http://127.0.0.1:4567')
    expect(openBrowser).not.toHaveBeenCalled()
    await ctx.fiber.dispose()
  })

  it('defers readiness publication until Loader settlement and drops it on failure or teardown', async () => {
    stageDist()
    const openBrowser = vi.fn(async () => {})
    internals.openBrowser = openBrowser
    // Settlement path: both actions wait for loader.await() so their consumers
    // can request the complete app immediately.
    const settled = new Context()
    settled.provide('webServer', fakeHttpServer().server)
    let release: () => void
    const settlement = new Promise<void>((resolve) => { release = resolve })
    provideLoader(settled, () => settlement)
    const log = vi.spyOn(console, 'log').mockImplementation(() => {})
    apply(settled, new Config({ openBrowser: true, printUrl: true, surfaceContext: true, trustedHosts: [] }))
    await new Promise(resolve => setTimeout(resolve, 0))
    expect(log).not.toHaveBeenCalled()
    expect(openBrowser).not.toHaveBeenCalled()
    release!()
    await new Promise(resolve => setTimeout(resolve, 0))
    expect(log).toHaveBeenCalledWith('dsh web: http://127.0.0.1:4567')
    expect(openBrowser).toHaveBeenCalledWith('http://127.0.0.1:4567')
    await settled.fiber.dispose()

    // Failed path: Loader reports the sibling failure; the app prints no URL
    // for a process that is about to exit.
    log.mockClear()
    openBrowser.mockClear()
    const failed = new Context()
    failed.provide('webServer', fakeHttpServer().server)
    provideLoader(failed, async () => { throw new Error('boot failed') })
    apply(failed, new Config({ openBrowser: true, printUrl: true, surfaceContext: true, trustedHosts: [] }))
    await new Promise(resolve => setTimeout(resolve, 0))
    expect(log).not.toHaveBeenCalled()
    expect(openBrowser).not.toHaveBeenCalled()
    await failed.fiber.dispose()

    // Torn-down path: settlement resolves after the webserver is gone — no
    // line, no crash.
    log.mockClear()
    openBrowser.mockClear()
    const torn = new Context()
    const child = torn.plugin((childCtx: Context) => {
      childCtx.provide('webServer', fakeHttpServer().server)
    })
    await child
    let releaseTorn: () => void
    const tornSettlement = new Promise<void>((resolve) => { releaseTorn = resolve })
    provideLoader(torn, () => tornSettlement)
    apply(torn, new Config({ openBrowser: true, printUrl: true, surfaceContext: true, trustedHosts: [] }))
    await child.dispose() // the webServer service goes away
    releaseTorn!()
    await new Promise(resolve => setTimeout(resolve, 0))
    expect(log).not.toHaveBeenCalled()
    expect(openBrowser).not.toHaveBeenCalled()
    await torn.fiber.dispose()
  })

  it('fails loud when the prompt section resolves against a portless webserver', async () => {
    stageDist()
    const ctx = new Context()
    // A webserver whose bound port is gone (torn down mid-request): the
    // section must throw, never render a URL with an undefined port.
    const { server } = fakeHttpServer()
    Object.defineProperty(server, 'port', { get: () => undefined })
    ctx.provide('webServer', server)
    apply(ctx, new Config({ openBrowser: false, printUrl: false, surfaceContext: true, trustedHosts: [] }))
    await ctx.plugin(SystemPrompt, { persona: '' })
    await new Promise(resolve => setTimeout(resolve, 0))
    await expect(ctx.systemPrompt.assemble()).rejects.toThrow('webServer service missing')
    await ctx.fiber.dispose()
  })

  it('leaves the bootstrap tap and the guard seat empty without desktop auth', async () => {
    stageDist()
    const ctx = new Context()
    const { server, applyTaps, guard } = fakeHttpServer()
    ctx.provide('webServer', server)
    apply(ctx, new Config({ printUrl: false, surfaceContext: false, trustedHosts: [] }))
    await new Promise(resolve => setTimeout(resolve, 0))
    expect(applyTaps('<head></head>')).toBe('<head></head>')
    // `dsh web` from a terminal must reach its routes unauthenticated.
    expect(guard()).toBeUndefined()
    await ctx.fiber.dispose()
  })

  it('guards every /api path with the launch token, whoever registered the route', async () => {
    stageDist()
    const ctx = new Context()
    const { server, guard } = fakeHttpServer()
    ctx.provide('webServer', server)
    ctx.provide(WEB_STARTUP_SERVICE, {
      trustedHosts: [],
      desktopToken: 'tok',
      desktopBootstrapNonce: 'nonce',
    })
    apply(ctx, new Config({ printUrl: false, surfaceContext: false, trustedHosts: [] }))
    await new Promise(resolve => setTimeout(resolve, 0))
    expect(ctx.get('webRuntime')).toEqual({
      lanAddresses: [],
      trustedHosts: [SHARE_INTERNAL_HOST],
    })
    const installed = guard()
    expect(installed).toBeDefined()
    // A patch-layer plugin's own namespace is authenticated without that
    // plugin implementing anything, because the seat covers the whole server.
    expect(installed?.(requestWith({}), '/api/usage-stats/balance')).toBe(401)
    expect(installed?.(requestWith({ cookie: 'dsh-token=tok' }), '/api/usage-stats/balance')).toBeUndefined()
    await ctx.fiber.dispose()
  })

  it('reports foreign /api routes as a warning instead of failing the launch', async () => {
    stageDist()
    const ctx = new Context()
    const { server } = fakeHttpServer('127.0.0.1', [
      { kind: 'prefix', path: '/api', owner: 'client-connection' },
      { kind: 'exact', path: '/api/usage-stats/balance', owner: 'unknown' },
    ])
    ctx.provide('webServer', server)
    ctx.provide(WEB_STARTUP_SERVICE, {
      trustedHosts: [],
      desktopToken: 'tok',
      desktopBootstrapNonce: 'nonce',
    })
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    provideLoader(ctx)
    apply(ctx, new Config({ printUrl: false, surfaceContext: false, trustedHosts: [] }))
    await new Promise(resolve => setTimeout(resolve, 0))
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('exact /api/usage-stats/balance owner=unknown'))
    await ctx.fiber.dispose()
  })

  it('injects a JSON-escaped nonce and never the token', async () => {
    stageDist()
    const ctx = new Context()
    const { server, applyTaps } = fakeHttpServer()
    ctx.provide('webServer', server)
    ctx.provide(WEB_STARTUP_SERVICE, {
      trustedHosts: [],
      desktopToken: "ab<'>secret",
      desktopBootstrapNonce: "ab<'>",
    })
    apply(ctx, new Config({ printUrl: false, surfaceContext: false, trustedHosts: [] }))
    await new Promise(resolve => setTimeout(resolve, 0))
    const injected = applyTaps('<head></head><body>shell</body>')
    expect(injected).toContain('window.__DSH_DESKTOP_BOOTSTRAP__')
    expect(injected).toContain('randomUUID')
    expect(injected).not.toContain('secret')
    expect(injected).not.toContain('__DSH_TOKEN__')
    expect(injected.startsWith('<head>')).toBe(true)
    await ctx.fiber.dispose()
  })

  it('prefixes html that has no head element', () => {
    expect(injectDesktopBootstrapScript('plain')).toMatch(/^<script>window\.__DSH_DESKTOP_BOOTSTRAP__/)
  })

  it('resolves the real built frontend dist through the package exports, failing loud unbuilt', () => {
    // The production resolver (not the test hook). A built checkout resolves
    // the frontend package's index.html; a dist-less one (the CI coverage
    // lane runs before any build) must fail with the build hint, never a
    // silent fallback.
    try {
      expect(originalResolve()).toMatch(/dist[/\\]index\.html$/)
    } catch (error) {
      expect((error as Error).message).toContain('frontend dist not built')
    }
  })

  it('fails loud when only one of the paired desktop env names is set', () => {
    stageDist()
    const ctx = new Context()
    ctx.provide('webServer', fakeHttpServer().server)
    ctx.provide(WEB_STARTUP_SERVICE, {
      trustedHosts: [],
      desktopToken: 'tok',
      desktopBootstrapNonce: '',
    })
    expect(() => apply(ctx, new Config({ printUrl: false, surfaceContext: false, trustedHosts: [] })))
      .toThrow('must both be set')
  })

  it('registers desktop control routes that the fake server can dispatch', async () => {
    stageDist()
    const ctx = new Context()
    const { server, handlers } = fakeHttpServer()
    ctx.provide('webServer', server)
    ctx.provide(WEB_STARTUP_SERVICE, {
      trustedHosts: [],
      desktopToken: 'tok',
      desktopBootstrapNonce: 'nonce',
    })
    apply(ctx, new Config({ printUrl: false, surfaceContext: false, trustedHosts: [] }))
    await new Promise(resolve => setTimeout(resolve, 0))
    const dispatch = createServer((req, res) => {
      const path = new URL(req.url ?? '/', 'http://x').pathname
      const handler = handlers.get(path)
      if (handler === undefined) {
        res.writeHead(404)
        res.end()
        return
      }
      handler(req, res)
    })
    await new Promise<void>(resolve => dispatch.listen(0, '127.0.0.1', resolve))
    const addr = dispatch.address()
    if (addr === null || typeof addr === 'string') throw new Error('expected tcp')
    const base = `http://127.0.0.1:${String(addr.port)}`
    try {
      const ready = await fetch(`${base}/__dshd_ready`, { method: 'POST' })
      expect(ready.status).toBe(401)
      const status = await fetch(`${base}/__dshd_status`)
      expect(status.status).toBe(401)
      const bootstrap = await fetch(`${base}/__dshd_bootstrap`, { method: 'GET' })
      expect(bootstrap.status).toBe(405)
    } finally {
      await new Promise<void>((resolve, reject) => {
        dispatch.close((error) => { if (error !== undefined) reject(error); else resolve() })
      })
      await ctx.fiber.dispose()
    }
  })

  it('does not open the default browser when the desktop bootstrap is active', async () => {
    stageDist()
    const ctx = new Context()
    ctx.provide('webServer', fakeHttpServer().server)
    ctx.provide(WEB_STARTUP_SERVICE, {
      trustedHosts: [],
      desktopToken: 'tok',
      desktopBootstrapNonce: 'nonce',
    })
    const openBrowser = vi.fn(async () => {})
    internals.openBrowser = openBrowser
    const log = vi.spyOn(console, 'log').mockImplementation(() => {})
    apply(ctx, new Config({ openBrowser: true, printUrl: true, surfaceContext: false, trustedHosts: [] }))
    await new Promise(resolve => setTimeout(resolve, 0))
    expect(openBrowser).not.toHaveBeenCalled()
    expect(log).toHaveBeenCalledWith('dsh web: http://127.0.0.1:4567')
    expect(log).not.toHaveBeenCalledWith('dsh web: opening the default browser; pass --no-open to disable')
    await ctx.fiber.dispose()
  })

  it.each([
    ['Error', new Error('no desktop'), 'no desktop'],
    ['non-Error', 'desktop unavailable', 'desktop unavailable'],
  ] as const)('keeps the server running and reports the manual URL when a browser failure is %s', async (_kind, failure, reason) => {
    stageDist()
    const ctx = new Context()
    ctx.provide('webServer', fakeHttpServer().server)
    internals.openBrowser = vi.fn(async () => { throw failure })
    const log = vi.spyOn(console, 'log').mockImplementation(() => {})
    const diagnostic = vi.spyOn(console, 'error').mockImplementation(() => {})
    apply(ctx, new Config({ openBrowser: true, printUrl: false, surfaceContext: false, trustedHosts: [] }))
    await new Promise(resolve => setTimeout(resolve, 0))
    expect(log).toHaveBeenCalledWith('dsh web: opening the default browser; pass --no-open to disable')
    expect(diagnostic).toHaveBeenCalledWith(
      `web-app: could not open the default browser because ${reason}; visit http://127.0.0.1:4567 manually`,
    )
    expect(ctx.get('webServer')).toBeDefined()
    await ctx.fiber.dispose()
  })

  it('scrubs the helper environment and reports helper spawn or exit failures', async () => {
    vi.stubEnv('DEEPSEEK_API_KEY', 'must-not-reach-browser')
    vi.stubEnv('DSH_HOME', '/must-not-reach-browser')
    const completed = launcher()
    vi.mocked(spawn).mockReturnValueOnce(completed)
    const completion = originalOpenBrowser('http://127.0.0.1:4567')
    const [command, args, options] = vi.mocked(spawn).mock.calls[0]!
    expect(command).toBe(process.execPath)
    expect(args).toEqual([
      '--input-type=module',
      '--eval', expect.stringContaining('await import('),
      '--', 'http://127.0.0.1:4567',
    ])
    expect(args?.[2]).toContain("if (process.platform === 'win32')")
    expect(args?.[2]).toContain('launcher.ref()')
    expect(options?.env).not.toHaveProperty('DEEPSEEK_API_KEY')
    expect(options?.env).not.toHaveProperty('DSH_HOME')
    expect(options?.env?.PATH).toBe(process.env.PATH)
    expect(options?.stdio).toEqual(['ignore', 'inherit', 'pipe'])
    completed.emit('close', 0)
    await expect(completion).resolves.toBeUndefined()
    expect(completed.listenerCount('error')).toBe(0)

    const completedWithStderr = launcher()
    vi.mocked(spawn).mockReturnValueOnce(completedWithStderr)
    const stderr = vi.spyOn(process.stderr, 'write').mockImplementation(() => true)
    const completionWithStderr = originalOpenBrowser('http://127.0.0.1:4567')
    completedWithStderr.stderr?.write('launcher note\n')
    completedWithStderr.emit('close', 0)
    await expect(completionWithStderr).resolves.toBeUndefined()
    expect(stderr).toHaveBeenCalledWith('launcher note\n')

    const failedWithReason = launcher()
    vi.mocked(spawn).mockReturnValueOnce(failedWithReason)
    const reasonFailure = originalOpenBrowser('http://127.0.0.1:4567')
    const reasonAssertion = expect(reasonFailure).rejects.toThrow('desktop unavailable')
    failedWithReason.stderr?.write('Error: desktop unavailable\n    at fixture')
    failedWithReason.emit('close', 1)
    await reasonAssertion

    const failed = launcher()
    vi.mocked(spawn).mockReturnValueOnce(failed)
    const failure = originalOpenBrowser('http://127.0.0.1:4567')
    const failureAssertion = expect(failure).rejects.toThrow('exited with code 3')
    await Promise.resolve()
    failed.emit('close', 3)
    await failureAssertion

    const errored = launcher()
    vi.mocked(spawn).mockReturnValueOnce(errored)
    const error = originalOpenBrowser('http://127.0.0.1:4567')
    const errorAssertion = expect(error).rejects.toThrow('spawn failed')
    await Promise.resolve()
    errored.emit('error', new Error('spawn failed'))
    await errorAssertion
    expect(errored.listenerCount('close')).toBe(0)
  })
})
