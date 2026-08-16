/**
 * Share gateway: pairing, revocable sessions, header stripping, Host rewrite,
 * HTTP/SSE/WebSocket proxy, and control-route authentication.
 */

import { createServer, request as httpRequest } from 'node:http'
import type { IncomingHttpHeaders, IncomingMessage, Server, ServerResponse } from 'node:http'
import { once } from 'node:events'
import { connect } from 'node:net'
import { afterEach, describe, expect, it } from 'vitest'
import { DesktopBootstrap } from '../src/desktop-bootstrap.ts'
import {
  handleShareControl,
  installShareGateway,
  listShareAddresses,
  isAllowedNearbyBind,
  SHARE_CONTROL_PATH,
  SHARE_COOKIE,
  SHARE_INTERNAL_HOST,
  ShareGateway,
  TICKET_TTL_MS,
} from '../src/share-gateway.ts'

const fixtures: Array<() => Promise<void>> = []

afterEach(async () => {
  while (fixtures.length > 0) await fixtures.pop()!()
})

function track(dispose: () => Promise<void> | void): void {
  fixtures.push(async () => { await dispose() })
}

async function listen(server: Server, beforeClose?: () => void): Promise<number> {
  await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve))
  const addr = server.address()
  if (addr === null || typeof addr === 'string') throw new Error('expected tcp')
  track(() => new Promise<void>((resolve, reject) => {
    beforeClose?.()
    server.closeAllConnections()
    server.close((error) => {
      if (error !== undefined && (error as NodeJS.ErrnoException).code !== 'ERR_SERVER_NOT_RUNNING') {
        reject(error)
        return
      }
      resolve()
    })
  }))
  return addr.port
}

interface Captured {
  host?: string
  token?: string
  forwarded?: string
  cookie?: string
  origin?: string
  path?: string
}

async function startSidecar(): Promise<{ port: number; captured: Captured[]; server: Server }> {
  const captured: Captured[] = []
  const server = createServer((req, res) => {
    captured.push({
      host: header(req, 'host'),
      token: header(req, 'x-dsh-token'),
      forwarded: header(req, 'x-forwarded-for'),
      cookie: header(req, 'cookie'),
      origin: header(req, 'origin'),
      path: req.url,
    })
    if (req.url === '/plugins/events') {
      res.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-cache' })
      res.write('data: ping\n\n')
      res.end()
      return
    }
    if (req.headers['set-cookie-probe'] === '1') {
      res.writeHead(200, { 'set-cookie': ['dsh-token=leaked; Path=/', 'other=1; Path=/'] })
      res.end('ok')
      return
    }
    if (req.url === '/only-token') {
      res.writeHead(200, { 'set-cookie': 'dsh-token=leaked; Path=/' })
      res.end('ok')
      return
    }
    if (req.url === '/array-header') {
      res.writeHead(200, { 'x-multi': ['a', 'b'] })
      res.end('ok')
      return
    }
    res.writeHead(200, { 'content-type': 'text/plain' })
    res.end('ok')
  })
  const upgraded = new Set<import('node:stream').Duplex>()
  server.on('upgrade', (req, socket) => {
    upgraded.add(socket)
    socket.on('close', () => { upgraded.delete(socket) })
    captured.push({
      host: header(req, 'host'),
      token: header(req, 'x-dsh-token'),
      path: req.url,
    })
    socket.write('HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\nUpgrade: dsh-test\r\n\r\n')
  })
  const port = await listen(server, () => {
    for (const socket of upgraded) socket.destroy()
  })
  return { port, captured, server }
}

function header(req: IncomingMessage, name: string): string | undefined {
  const value = req.headers[name]
  return typeof value === 'string' ? value : undefined
}

async function startGateway(
  sidecarPort: number,
  interfaces?: NodeJS.Dict<import('node:os').NetworkInterfaceInfo[]>,
): Promise<ShareGateway> {
  const gateway = new ShareGateway({
    launchToken: 'launch-token',
    sidecarPort,
    interfaces: interfaces ?? {
      lo0: [{ family: 'IPv4', internal: true, address: '127.0.0.1' } as never],
      en0: [{ family: 'IPv4', internal: false, address: '192.168.1.8' } as never],
      docker0: [{ family: 'IPv4', internal: false, address: '172.17.0.1' } as never],
    },
  })
  await gateway.listenLoopback()
  track(async () => { await gateway.dispose() })
  return gateway
}

function cookieFrom(response: Response): string | undefined {
  const raw = response.headers.getSetCookie()[0]
  if (raw === undefined) return undefined
  const match = new RegExp(`${SHARE_COOKIE}=([^;]+)`).exec(raw)
  return match?.[1]
}

function cookieFromSetCookie(values: string | string[] | undefined): string | undefined {
  const list = values === undefined ? [] : Array.isArray(values) ? values : [values]
  const raw = list[0]
  if (raw === undefined) return undefined
  const match = new RegExp(`${SHARE_COOKIE}=([^;]+)`).exec(raw)
  return match?.[1]
}

function rawRequest(options: {
  port: number
  path: string
  method?: string
  headers?: Record<string, string>
}): Promise<{ status: number; headers: IncomingHttpHeaders; body: string }> {
  return new Promise((resolve, reject) => {
    const req = httpRequest({
      hostname: '127.0.0.1',
      port: options.port,
      path: options.path,
      method: options.method ?? 'GET',
      headers: options.headers,
    }, (res) => {
      const chunks: Buffer[] = []
      res.on('data', (chunk: Buffer) => { chunks.push(chunk) })
      res.on('end', () => {
        resolve({
          status: res.statusCode ?? 0,
          headers: res.headers,
          body: Buffer.concat(chunks).toString(),
        })
      })
    })
    req.on('error', reject)
    req.end()
  })
}

async function upgradeOnce(
  port: number,
  requestLine: string,
  headers: Record<string, string>,
  extra = '',
): Promise<string> {
  const socket = connect(port, '127.0.0.1')
  socket.on('error', () => {})
  await once(socket, 'connect')
  const headerLines = Object.entries(headers).map(([name, value]) => {
    const label = name === 'host' ? 'Host' : name === 'cookie' ? 'Cookie' : name
    return `${label}: ${value}`
  })
  const data = Promise.race([
    once(socket, 'data') as Promise<[Buffer]>,
    once(socket, 'close').then(() => [Buffer.alloc(0)] as [Buffer]),
  ])
  socket.write([
    requestLine,
    ...headerLines,
    'Connection: Upgrade',
    'Upgrade: websocket',
    'Sec-WebSocket-Version: 13',
    'Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==',
    '',
    '',
  ].join('\r\n') + extra)
  const [buf] = await data
  socket.destroy()
  return buf.toString()
}

describe('listShareAddresses', () => {
  it('prefers RFC1918 Wi-Fi literals and skips Docker plus link-local', () => {
    expect(listShareAddresses({
      lo0: [{ family: 'IPv4', internal: true, address: '127.0.0.1' } as never],
      en0: [{ family: 'IPv4', internal: false, address: '192.168.1.8' } as never],
      en1: [{ family: 'IPv4', internal: false, address: '10.0.0.4' } as never],
      utun0: undefined,
      docker0: [{ family: 'IPv4', internal: false, address: '172.17.0.1' } as never],
      en2: [{ family: 'IPv4', internal: false, address: '169.254.1.1' } as never],
      en3: [{ family: 'IPv6', internal: false, address: 'fe80::1' } as never],
      en4: [{ family: 'IPv4', internal: false, address: '172.20.0.5' } as never],
      veth0: [{ family: 'IPv4', internal: false, address: '10.88.0.1' } as never],
    }).map(item => item.address)).toEqual(['192.168.1.8', '10.0.0.4', '172.20.0.5'])
  })

  it('sorts two Wi-Fi literals and ranks a public address last', () => {
    expect(listShareAddresses({
      en0: [{ family: 'IPv4', internal: false, address: '192.168.2.1' } as never],
      en1: [{ family: 'IPv4', internal: false, address: '192.168.1.1' } as never],
      en2: [{ family: 'IPv4', internal: false, address: '8.8.8.8' } as never],
      br0: [{ family: 'IPv4', internal: false, address: '172.16.0.2' } as never],
    }).map(item => item.address)).toEqual(['192.168.1.1', '192.168.2.1', '172.16.0.2', '8.8.8.8'])
  })
})

describe('isAllowedNearbyBind', () => {
  const listed = [{ address: '192.168.1.8', iface: 'en0' }]

  it('allows a listed LAN literal and loopback, and refuses 0.0.0.0', () => {
    expect(isAllowedNearbyBind('192.168.1.8', listed)).toBe(true)
    expect(isAllowedNearbyBind('127.0.0.1', listed)).toBe(true)
    expect(isAllowedNearbyBind('0.0.0.0', listed)).toBe(false)
    expect(isAllowedNearbyBind('8.8.8.8', listed)).toBe(false)
  })
})

describe('loopback pairing', () => {
  it('pairs during the open window, injects the launch token, and rewrites Host to loopback', async () => {
    const sidecar = await startSidecar()
    const gateway = await startGateway(sidecar.port)
    const status = gateway.status()
    const port = status.loopbackPort!
    const denied = await fetch(`http://127.0.0.1:${String(port)}/`)
    expect(denied.status).toBe(401)
    expect(await denied.text()).toContain('请回到 Desktop')

    gateway.openLoopbackPairing()
    const paired = await fetch(`http://127.0.0.1:${String(port)}/`, {
      headers: { 'x-forwarded-for': '1.2.3.4', 'x-dsh-token': 'stolen', cookie: 'dsh-token=stolen' },
    })
    expect(paired.status).toBe(200)
    expect(await paired.text()).toBe('ok')
    const cookie = cookieFrom(paired)
    expect(cookie).toBeDefined()
    expect(paired.headers.getSetCookie()[0]).not.toContain('Secure')
    expect(sidecar.captured.at(-1)).toMatchObject({
      host: `127.0.0.1:${String(sidecar.port)}`,
      token: 'launch-token',
      forwarded: undefined,
      cookie: undefined,
    })

    const again = await fetch(`http://127.0.0.1:${String(port)}/api/session.list`, {
      headers: { cookie: `${SHARE_COOKIE}=${cookie!}` },
    })
    expect(again.status).toBe(200)
    expect(sidecar.captured.at(-1)?.origin).toBe(`http://127.0.0.1:${String(sidecar.port)}`)
  })

  it('rejects a cross-site marker and a mismatched Origin', async () => {
    const sidecar = await startSidecar()
    const gateway = await startGateway(sidecar.port)
    gateway.openLoopbackPairing()
    const port = gateway.status().loopbackPort!
    const cross = await fetch(`http://127.0.0.1:${String(port)}/`, {
      headers: { 'sec-fetch-site': 'cross-site' },
    })
    expect(cross.status).toBe(403)
    const origin = await fetch(`http://127.0.0.1:${String(port)}/`, {
      headers: { origin: 'http://evil.test' },
    })
    expect(origin.status).toBe(403)
  })
})

describe('nearby tickets', () => {
  it('does not consume a ticket on GET, then pairs on POST and revokes on disable', async () => {
    const sidecar = await startSidecar()
    const gateway = await startGateway(sidecar.port)
    const nearby = await gateway.setNearby(true, '127.0.0.1')
    expect(nearby).not.toBeNull()
    const url = gateway.status().nearbyTicketUrl!
    const scan = await fetch(url)
    expect(scan.status).toBe(200)
    expect(scan.headers.get('referrer-policy')).toBe('no-referrer')
    expect(await scan.text()).toContain('打开')
    const scanAgain = await fetch(url)
    expect(scanAgain.status).toBe(200)

    const head = await fetch(url, { method: 'HEAD' })
    expect(head.status).toBe(200)
    expect(await head.text()).toBe('')

    const put = await fetch(url, { method: 'PUT' })
    expect(put.status).toBe(405)

    const paired = await fetch(url, { method: 'POST', redirect: 'manual' })
    expect(paired.status).toBe(303)
    const cookie = cookieFrom(paired)
    expect(cookie).toBeDefined()
    const replay = await fetch(url, { method: 'POST', redirect: 'manual' })
    expect(replay.status).toBe(410)

    const page = await fetch(`http://127.0.0.1:${String(nearby!.port)}/`, {
      headers: { cookie: `${SHARE_COOKIE}=${cookie!}` },
    })
    expect(page.status).toBe(200)
    expect(sidecar.captured.at(-1)?.host).toBe(`${SHARE_INTERNAL_HOST}:${String(sidecar.port)}`)

    await gateway.setNearby(false)
    const after = await fetch(`http://127.0.0.1:${String(nearby!.port)}/`, {
      headers: { cookie: `${SHARE_COOKIE}=${cookie!}` },
    }).catch((error: unknown) => error)
    expect(after).toBeInstanceOf(Error)
  })

  it('refuses a ticket posted to the wrong listener', async () => {
    const sidecar = await startSidecar()
    const gateway = await startGateway(sidecar.port)
    await gateway.setNearby(true, '127.0.0.1')
    const url = gateway.status().nearbyTicketUrl!
    const loopback = gateway.status().loopbackPort!
    const ticketPath = new URL(url).pathname
    const wrong = await fetch(`http://127.0.0.1:${String(loopback)}${ticketPath}`, {
      method: 'POST',
      redirect: 'manual',
    })
    expect(wrong.status).toBe(410)
  })
})

describe('tailscale audience', () => {
  it('pairs on the loopback listen with the MagicDNS Host and sets Secure', async () => {
    const sidecar = await startSidecar()
    const gateway = await startGateway(sidecar.port)
    gateway.setTailscaleAudience('mac.ts.net')
    const url = gateway.status().tailscaleTicketUrl!
    expect(url.startsWith('https://mac.ts.net/p/')).toBe(true)
    const port = gateway.status().loopbackPort!
    const path = new URL(url).pathname
    const paired = await rawRequest({
      port,
      path,
      method: 'POST',
      headers: { host: 'mac.ts.net' },
    })
    expect(paired.status).toBe(303)
    const setCookie = paired.headers['set-cookie']
    expect(Array.isArray(setCookie) ? setCookie[0] : setCookie).toContain('Secure')
    const cookie = cookieFromSetCookie(setCookie)!
    const page = await rawRequest({
      port,
      path: '/',
      headers: { host: 'mac.ts.net', cookie: `${SHARE_COOKIE}=${cookie}` },
    })
    expect(page.status).toBe(200)
    expect(sidecar.captured.at(-1)?.host).toBe(`${SHARE_INTERNAL_HOST}:${String(sidecar.port)}`)

    gateway.setTailscaleAudience(null)
    const dead = await rawRequest({
      port,
      path: '/',
      headers: { host: 'mac.ts.net', cookie: `${SHARE_COOKIE}=${cookie}` },
    })
    expect(dead.status).toBe(401)
  })
})

describe('proxy', () => {
  it('streams SSE, strips launch Set-Cookie, and blocks desktop control paths', async () => {
    const sidecar = await startSidecar()
    const gateway = await startGateway(sidecar.port)
    gateway.openLoopbackPairing()
    const port = gateway.status().loopbackPort!
    const paired = await fetch(`http://127.0.0.1:${String(port)}/`)
    const cookie = cookieFrom(paired)!

    const sse = await fetch(`http://127.0.0.1:${String(port)}/plugins/events`, {
      headers: { cookie: `${SHARE_COOKIE}=${cookie}` },
    })
    expect(sse.status).toBe(200)
    expect(sse.headers.get('content-type')).toContain('text/event-stream')
    const reader = sse.body!.getReader()
    const first = await reader.read()
    expect(new TextDecoder().decode(first.value)).toContain('ping')
    await reader.cancel()

    const leak = await fetch(`http://127.0.0.1:${String(port)}/`, {
      headers: { cookie: `${SHARE_COOKIE}=${cookie}`, 'set-cookie-probe': '1' },
    })
    const setCookie = leak.headers.getSetCookie().join(';')
    expect(setCookie).not.toContain('dsh-token=')
    expect(setCookie).toContain('other=1')

    const blocked = await fetch(`http://127.0.0.1:${String(port)}/__dshd_bootstrap`, {
      headers: { cookie: `${SHARE_COOKIE}=${cookie}` },
    })
    expect(blocked.status).toBe(404)
  })

  it('proxies WebSocket upgrades and refuses unpaired upgrades', async () => {
    const sidecar = await startSidecar()
    const gateway = await startGateway(sidecar.port)
    const port = gateway.status().loopbackPort!
    const refused = connect(port, '127.0.0.1')
    refused.on('error', () => {})
    await once(refused, 'connect')
    const closedOrData = Promise.race([once(refused, 'close'), once(refused, 'data')])
    refused.write([
      'GET /api/events.mux HTTP/1.1',
      `Host: 127.0.0.1:${String(port)}`,
      'Connection: Upgrade',
      'Upgrade: websocket',
      'Sec-WebSocket-Version: 13',
      'Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==',
      '',
      '',
    ].join('\r\n'))
    await closedOrData
    refused.destroy()

    gateway.openLoopbackPairing()
    const paired = await fetch(`http://127.0.0.1:${String(port)}/`)
    const cookie = cookieFrom(paired)!
    const socket = connect(port, '127.0.0.1')
    await once(socket, 'connect')
    const data = once(socket, 'data')
    socket.write([
      'GET /api/events.mux HTTP/1.1',
      `Host: 127.0.0.1:${String(port)}`,
      `Cookie: ${SHARE_COOKIE}=${cookie}`,
      'Connection: Upgrade',
      'Upgrade: websocket',
      'Sec-WebSocket-Version: 13',
      'Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==',
      '',
      '',
    ].join('\r\n'))
    const [buf] = await data as [Buffer]
    expect(buf.toString()).toContain('101 Switching Protocols')
    expect(sidecar.captured.at(-1)?.token).toBe('launch-token')
    socket.destroy()
  })
})

describe('control route', () => {
  it('accepts only the header token on loopback Host', async () => {
    const sidecar = await startSidecar()
    const gateway = await startGateway(sidecar.port)
    const server = createServer((req, res) => {
      void handleShareControl(req, res, 'launch-token', gateway)
    })
    const port = await listen(server)

    const noAuth = await fetch(`http://127.0.0.1:${String(port)}${SHARE_CONTROL_PATH}`, { method: 'POST' })
    expect(noAuth.status).toBe(401)
    const cookieOnly = await fetch(`http://127.0.0.1:${String(port)}${SHARE_CONTROL_PATH}`, {
      method: 'POST',
      headers: { cookie: 'dsh-token=launch-token' },
    })
    expect(cookieOnly.status).toBe(401)
    const get = await fetch(`http://127.0.0.1:${String(port)}${SHARE_CONTROL_PATH}`)
    expect(get.status).toBe(405)
    const badJson = await fetch(`http://127.0.0.1:${String(port)}${SHARE_CONTROL_PATH}`, {
      method: 'POST',
      headers: { 'x-dsh-token': 'launch-token', 'content-type': 'application/json' },
      body: '{',
    })
    expect(badJson.status).toBe(400)
    const unknown = await fetch(`http://127.0.0.1:${String(port)}${SHARE_CONTROL_PATH}`, {
      method: 'POST',
      headers: { 'x-dsh-token': 'launch-token', 'content-type': 'application/json' },
      body: JSON.stringify({ op: 'nope' }),
    })
    expect(unknown.status).toBe(400)
    const opened = await fetch(`http://127.0.0.1:${String(port)}${SHARE_CONTROL_PATH}`, {
      method: 'POST',
      headers: { 'x-dsh-token': 'launch-token', 'content-type': 'application/json' },
      body: JSON.stringify({ op: 'openLoopback' }),
    })
    expect(opened.status).toBe(200)
    expect(await opened.json()).toMatchObject({ loopbackPort: gateway.status().loopbackPort })

    const nearby = await fetch(`http://127.0.0.1:${String(port)}${SHARE_CONTROL_PATH}`, {
      method: 'POST',
      headers: { 'x-dsh-token': 'launch-token', 'content-type': 'application/json' },
      body: JSON.stringify({ op: 'setNearby', enabled: true, bindAddress: '127.0.0.1' }),
    })
    expect(nearby.status).toBe(200)
    const ts = await fetch(`http://127.0.0.1:${String(port)}${SHARE_CONTROL_PATH}`, {
      method: 'POST',
      headers: { 'x-dsh-token': 'launch-token', 'content-type': 'application/json' },
      body: JSON.stringify({ op: 'setTailscaleAudience', audience: 'mac.ts.net' }),
    })
    expect(ts.status).toBe(200)
    const huge = 'x'.repeat(5000)
    const tooBig = await fetch(`http://127.0.0.1:${String(port)}${SHARE_CONTROL_PATH}`, {
      method: 'POST',
      headers: { 'x-dsh-token': 'launch-token', 'content-type': 'application/json' },
      body: huge,
    })
    expect(tooBig.status).toBe(413)
  })

  it('installs on the sidecar composition and reports status', async () => {
    const sidecar = createServer((req, res) => {
      const path = new URL(req.url ?? '/', 'http://x').pathname
      if (path === SHARE_CONTROL_PATH && control !== undefined) {
        control(req, res)
        return
      }
      res.writeHead(200)
      res.end('sid')
    })
    let control: ((req: IncomingMessage, res: ServerResponse) => void) | undefined
    const sidecarPort = await listen(sidecar)
    const dispose = installShareGateway({
      webServer: {
        port: sidecarPort,
        register: (route: { handler: (req: IncomingMessage, res: ServerResponse) => void }) => {
          control = route.handler
          return () => { control = undefined }
        },
      },
    } as never, new DesktopBootstrap('tok', 'nonce'))
    track(async () => { await dispose() })
    await viWait()
    const response = await fetch(`http://127.0.0.1:${String(sidecarPort)}${SHARE_CONTROL_PATH}`, {
      method: 'POST',
      headers: { 'x-dsh-token': 'tok', 'content-type': 'application/json' },
      body: JSON.stringify({ op: 'status' }),
    })
    expect(response.status).toBe(200)
    const body = await response.json() as { loopbackPort: number }
    expect(body.loopbackPort).toBeGreaterThan(0)
  })
})

describe('gateway lifecycle', () => {
  it('returns the same loopback port, fails nearby without a LAN address, and rejects listen after dispose', async () => {
    const sidecar = await startSidecar()
    const gateway = new ShareGateway({
      launchToken: 't',
      sidecarPort: sidecar.port,
      interfaces: { lo0: [{ family: 'IPv4', internal: true, address: '127.0.0.1' } as never] },
    })
    expect(gateway.status().loopbackPort).toBeNull()
    const first = await gateway.listenLoopback()
    expect(await gateway.listenLoopback()).toBe(first)
    await expect(gateway.setNearby(true)).rejects.toThrow('no LAN address')
    await gateway.dispose()
    await expect(gateway.listenLoopback()).rejects.toThrow('disposed')
  })

  it('rebinds nearby when the address changes and keeps the port when it does not', async () => {
    const sidecar = await startSidecar()
    const gateway = await startGateway(sidecar.port)
    const first = await gateway.setNearby(true, '127.0.0.1')
    const same = await gateway.setNearby(true, '127.0.0.1')
    expect(same).toEqual(first)
    const off = await gateway.setNearby(false)
    expect(off).toBeNull()
  })

  it('binds the preferred LAN literal when setNearby omits an address', async () => {
    const sidecar = await startSidecar()
    const gateway = new ShareGateway({
      launchToken: 't',
      sidecarPort: sidecar.port,
      interfaces: {
        en0: [{ family: 'IPv4', internal: false, address: '127.0.0.1' } as never],
      },
    })
    await gateway.listenLoopback()
    track(async () => { await gateway.dispose() })
    const nearby = await gateway.setNearby(true)
    expect(nearby?.bindAddress).toBe('127.0.0.1')
  })

  it('rejects a listen on an address the host does not own', async () => {
    const sidecar = await startSidecar()
    const gateway = await startGateway(sidecar.port)
    await expect(gateway.setNearby(true, '8.8.8.8')).rejects.toThrow(
      'bind address is not a listed LAN address',
    )
    await expect(gateway.setNearby(true, '0.0.0.0')).rejects.toThrow(
      'bind address is not a listed LAN address',
    )
  })

  it('surfaces the OS error when a listed address cannot be bound', async () => {
    const sidecar = await startSidecar()
    const gateway = await startGateway(sidecar.port, {
      en0: [{ family: 'IPv4', internal: false, address: '203.0.113.1' } as never],
    })
    await expect(gateway.setNearby(true, '203.0.113.1')).rejects.toThrow()
  })
})

describe('pairing edges', () => {
  it('serves GET /p, rejects a cross-site POST, and remints after consume or expiry', async () => {
    let now = 1_000
    const sidecar = await startSidecar()
    const gateway = new ShareGateway({
      launchToken: 'launch-token',
      sidecarPort: sidecar.port,
      now: () => now,
      interfaces: {
        en0: [{ family: 'IPv4', internal: false, address: '127.0.0.1' } as never],
      },
    })
    await gateway.listenLoopback()
    track(async () => { await gateway.dispose() })
    const nearby = await gateway.setNearby(true, '127.0.0.1')
    const port = nearby!.port
    const bare = await fetch(`http://127.0.0.1:${String(port)}/p`)
    expect(bare.status).toBe(200)
    const url = gateway.status().nearbyTicketUrl!
    const blocked = await fetch(url, {
      method: 'POST',
      redirect: 'manual',
      headers: { 'sec-fetch-site': 'cross-site' },
    })
    expect(blocked.status).toBe(403)
    const paired = await fetch(url, { method: 'POST', redirect: 'manual' })
    expect(paired.status).toBe(303)
    const reminted = gateway.status().nearbyTicketUrl!
    expect(reminted).not.toBe(url)
    now += TICKET_TTL_MS + 1
    const afterExpiry = gateway.status().nearbyTicketUrl!
    expect(afterExpiry).not.toBe(reminted)
  })

  it('keeps a loopback session when nearby turns off, and refuses a loopback cookie on nearby', async () => {
    const sidecar = await startSidecar()
    const gateway = await startGateway(sidecar.port)
    gateway.openLoopbackPairing()
    const loopback = gateway.status().loopbackPort!
    const paired = await fetch(`http://127.0.0.1:${String(loopback)}/`)
    const cookie = cookieFrom(paired)!
    const nearby = await gateway.setNearby(true, '127.0.0.1')
    const wrong = await fetch(`http://127.0.0.1:${String(nearby!.port)}/`, {
      headers: { cookie: `${SHARE_COOKIE}=${cookie}` },
    })
    expect(wrong.status).toBe(401)
    await gateway.setNearby(false)
    const still = await fetch(`http://127.0.0.1:${String(loopback)}/`, {
      headers: { cookie: `${SHARE_COOKIE}=${cookie}` },
    })
    expect(still.status).toBe(200)
  })

  it('rejects a nearby session whose Host does not match the bind', async () => {
    const sidecar = await startSidecar()
    const gateway = await startGateway(sidecar.port)
    const nearby = await gateway.setNearby(true, '127.0.0.1')
    const url = gateway.status().nearbyTicketUrl!
    const paired = await fetch(url, { method: 'POST', redirect: 'manual' })
    const cookie = cookieFrom(paired)!
    const mismatched = await rawRequest({
      port: nearby!.port,
      path: '/',
      headers: { host: `10.0.0.4:${String(nearby!.port)}`, cookie: `${SHARE_COOKIE}=${cookie}` },
    })
    expect(mismatched.status).toBe(401)
  })

  it('clears Tailscale with an empty audience and matches a default http port', async () => {
    const sidecar = await startSidecar()
    const gateway = await startGateway(sidecar.port)
    gateway.setTailscaleAudience('mac.ts.net:80')
    const port = gateway.status().loopbackPort!
    const path = new URL(gateway.status().tailscaleTicketUrl!).pathname
    const paired = await rawRequest({
      port,
      path,
      method: 'POST',
      headers: { host: 'mac.ts.net' },
    })
    expect(paired.status).toBe(303)
    gateway.setTailscaleAudience('')
    expect(gateway.status().tailscaleAudience).toBeNull()
    gateway.setTailscaleAudience('mac.ts.net')
    const first = gateway.status().tailscaleTicketUrl
    const consumedPath = new URL(first!).pathname
    const consumed = await rawRequest({
      port,
      path: consumedPath,
      method: 'POST',
      headers: { host: 'mac.ts.net' },
    })
    expect(gateway.status().tailscaleTicketUrl).not.toBe(first)
    const cookie = cookieFromSetCookie(consumed.headers['set-cookie'])!
    gateway.setTailscaleAudience('mac.ts.net:8443')
    const wrongPort = await rawRequest({
      port,
      path: '/',
      headers: { host: 'mac.ts.net', cookie: `${SHARE_COOKIE}=${cookie}` },
    })
    expect(wrongPort.status).toBe(401)
  })

  it('returns 410 for an expired Host and 403 for a broken Origin', async () => {
    const sidecar = await startSidecar()
    const gateway = await startGateway(sidecar.port)
    await gateway.setNearby(true, '127.0.0.1')
    const url = gateway.status().nearbyTicketUrl!
    const expiredHost = await rawRequest({
      port: Number(new URL(url).port),
      path: new URL(url).pathname,
      method: 'POST',
      headers: { host: '[' },
    })
    expect(expiredHost.status).toBe(410)
    gateway.openLoopbackPairing()
    const loopback = gateway.status().loopbackPort!
    const origin = await fetch(`http://127.0.0.1:${String(loopback)}/`, {
      headers: { origin: 'not-a-url' },
    })
    expect(origin.status).toBe(403)
    const emptyOriginHost = await rawRequest({
      port: loopback,
      path: '/',
      headers: { origin: 'http://127.0.0.1' },
    })
    expect(emptyOriginHost.status).toBe(403)
  })

  it('ignores a malformed share cookie and a cookie for another name', async () => {
    let now = 1_000
    const sidecar = await startSidecar()
    const gateway = new ShareGateway({
      launchToken: 'launch-token',
      sidecarPort: sidecar.port,
      now: () => now,
      interfaces: {
        en0: [{ family: 'IPv4', internal: false, address: '192.168.1.8' } as never],
      },
    })
    await gateway.listenLoopback()
    track(async () => { await gateway.dispose() })
    gateway.openLoopbackPairing(50)
    const port = gateway.status().loopbackPort!
    const malformed = await fetch(`http://127.0.0.1:${String(port)}/`, {
      headers: { cookie: `${SHARE_COOKIE}=%E0%A4%A; other=1` },
    })
    expect(malformed.status).toBe(200)
    now = 2_000
    const unpaired = await fetch(`http://127.0.0.1:${String(port)}/`, {
      headers: { cookie: 'other=1; =novalue' },
    })
    expect(unpaired.status).toBe(401)
    const head = await fetch(`http://127.0.0.1:${String(port)}/`, { method: 'HEAD' })
    expect(head.status).toBe(401)
    expect(await head.text()).toBe('')
    const blocked = await fetch(`http://127.0.0.1:${String(port)}/__dshd`)
    expect(blocked.status).toBe(404)
    const nested = await fetch(`http://127.0.0.1:${String(port)}/__dshd/x`)
    expect(nested.status).toBe(404)
    now = 1_000
    gateway.openLoopbackPairing(50)
    const localhost = await rawRequest({
      port,
      path: '/',
      headers: { host: `localhost:${String(port)}` },
    })
    expect(localhost.status).toBe(200)
    now = 1_000
    gateway.openLoopbackPairing(50)
    const ipv6 = await new Promise<{ status: number }>((resolve, reject) => {
      const socket = connect(port, '127.0.0.1')
      socket.on('error', reject)
      socket.on('connect', () => {
        socket.write(`GET / HTTP/1.1\r\nHost: [::1]:${String(port)}\r\nConnection: close\r\n\r\n`)
      })
      const chunks: Buffer[] = []
      socket.on('data', (chunk: Buffer) => { chunks.push(chunk) })
      socket.on('end', () => {
        const head = Buffer.concat(chunks).toString().split('\r\n')[0] ?? ''
        const status = Number(head.split(' ')[1] ?? '0')
        resolve({ status })
      })
    })
    expect(ipv6.status).toBe(200)
    now = 1_000
    gateway.openLoopbackPairing(50)
    const badHost = await rawRequest({
      port,
      path: '/',
      headers: { host: '[' },
    })
    expect(badHost.status).toBe(401)
    now = 1_000
    gateway.openLoopbackPairing(50)
    const bareV6 = await rawRequest({
      port,
      path: '/',
      headers: { host: '::1' },
    })
    expect(bareV6.status).toBe(200)
    now = 1_000
    gateway.openLoopbackPairing(50)
    const v6port = await rawRequest({
      port,
      path: '/',
      headers: { host: '::1:80' },
    })
    expect(v6port.status).toBe(200)
  })
})

describe('proxy edges', () => {
  it('answers 502 when the sidecar is down and strips an all-token Set-Cookie', async () => {
    const sidecar = await startSidecar()
    const gateway = await startGateway(sidecar.port)
    gateway.openLoopbackPairing()
    const port = gateway.status().loopbackPort!
    const paired = await fetch(`http://127.0.0.1:${String(port)}/`)
    const cookie = cookieFrom(paired)!
    const stripped = await fetch(`http://127.0.0.1:${String(port)}/only-token`, {
      headers: { cookie: `${SHARE_COOKIE}=${cookie}` },
    })
    expect(stripped.headers.getSetCookie().join(';')).not.toContain('dsh-token=')
    const multi = await fetch(`http://127.0.0.1:${String(port)}/array-header`, {
      headers: { cookie: `${SHARE_COOKIE}=${cookie}` },
    })
    expect(multi.status).toBe(200)
    sidecar.server.closeAllConnections()
    await new Promise<void>((resolve, reject) => {
      sidecar.server.close((error) => { if (error !== undefined) reject(error); else resolve() })
    })
    const down = await fetch(`http://127.0.0.1:${String(port)}/`, {
      headers: { cookie: `${SHARE_COOKIE}=${cookie}` },
    })
    expect(down.status).toBe(502)
  })

  it('refuses unpaired upgrades of /p and /__dshd, and a cross-site upgrade', async () => {
    const sidecar = await startSidecar()
    const gateway = await startGateway(sidecar.port)
    const port = gateway.status().loopbackPort!
    const pairUpgrade = await upgradeOnce(port, 'GET /p/abc HTTP/1.1', { host: `127.0.0.1:${String(port)}` })
    expect(pairUpgrade).toContain('404')
    const controlUpgrade = await upgradeOnce(port, 'GET /__dshd_status HTTP/1.1', { host: `127.0.0.1:${String(port)}` })
    expect(controlUpgrade).toContain('404')
    const cross = await upgradeOnce(port, 'GET /api/events.mux HTTP/1.1', {
      host: `127.0.0.1:${String(port)}`,
      'sec-fetch-site': 'cross-site',
    })
    expect(cross).toContain('403')
  })

  it('forwards leftover upgrade bytes and surfaces a sidecar HTTP response to upgrade', async () => {
    const sidecar = createServer((_req, res) => {
      res.writeHead(400, { 'x-multi': ['a', 'b'] })
      res.end('nope')
    })
    const upgraded = new Set<import('node:stream').Duplex>()
    sidecar.on('upgrade', (_req, socket) => {
      upgraded.add(socket)
      socket.on('close', () => { upgraded.delete(socket) })
      socket.write('HTTP/1.1 101 Switching Protocols\r\nSet-Cookie: a=1\r\nSet-Cookie: b=2\r\nConnection: Upgrade\r\nUpgrade: dsh-test\r\n\r\nleftover')
    })
    const sidecarPort = await listen(sidecar, () => {
      for (const socket of upgraded) socket.destroy()
    })
    const gateway = await startGateway(sidecarPort)
    gateway.openLoopbackPairing()
    const port = gateway.status().loopbackPort!
    const paired = await fetch(`http://127.0.0.1:${String(port)}/`)
    const cookie = cookieFrom(paired)!
    const upgradeBody = await upgradeOnce(port, 'GET /api/events.mux HTTP/1.1', {
      host: `127.0.0.1:${String(port)}`,
      cookie: `${SHARE_COOKIE}=${cookie}`,
    }, 'extra-head')
    expect(upgradeBody).toContain('101 Switching Protocols')
    expect(upgradeBody).toContain('leftover')

    const refused = createServer((_req, res) => {
      res.writeHead(404, { 'set-cookie': ['a=1', 'b=2'] })
      res.end('no upgrade')
    })
    const refusedSockets = new Set<import('node:stream').Duplex>()
    refused.on('connection', (socket) => {
      refusedSockets.add(socket)
      socket.on('close', () => { refusedSockets.delete(socket) })
    })
    const refusedPort = await listen(refused, () => {
      for (const socket of refusedSockets) socket.destroy()
    })
    const gateway2 = await startGateway(refusedPort)
    gateway2.openLoopbackPairing()
    const port2 = gateway2.status().loopbackPort!
    const paired2 = await fetch(`http://127.0.0.1:${String(port2)}/`)
    const cookie2 = cookieFrom(paired2)!
    const response = await upgradeOnce(port2, 'GET /api/events.mux HTTP/1.1', {
      host: `127.0.0.1:${String(port2)}`,
      cookie: `${SHARE_COOKIE}=${cookie2}`,
    })
    expect(response).toContain('404')
  })

  it('destroys an upgrade when the sidecar port is closed', async () => {
    const sidecar = await startSidecar()
    const gateway = await startGateway(sidecar.port)
    gateway.openLoopbackPairing()
    const port = gateway.status().loopbackPort!
    const paired = await fetch(`http://127.0.0.1:${String(port)}/`)
    const cookie = cookieFrom(paired)!
    sidecar.server.closeAllConnections()
    await new Promise<void>((resolve, reject) => {
      sidecar.server.close((error) => { if (error !== undefined) reject(error); else resolve() })
    })
    const socket = connect(port, '127.0.0.1')
    await once(socket, 'connect')
    const closed = once(socket, 'close')
    socket.write([
      'GET /api/events.mux HTTP/1.1',
      `Host: 127.0.0.1:${String(port)}`,
      `Cookie: ${SHARE_COOKIE}=${cookie}`,
      'Connection: Upgrade',
      'Upgrade: websocket',
      'Sec-WebSocket-Version: 13',
      'Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==',
      '',
      '',
    ].join('\r\n'))
    await closed
  })
})

describe('control edges', () => {
  it('treats an empty body as status and validates each op', async () => {
    const sidecar = await startSidecar()
    const gateway = new ShareGateway({
      launchToken: 'launch-token',
      sidecarPort: sidecar.port,
      interfaces: { lo0: [{ family: 'IPv4', internal: true, address: '127.0.0.1' } as never] },
    })
    await gateway.listenLoopback()
    track(async () => { await gateway.dispose() })
    const server = createServer((req, res) => {
      void handleShareControl(req, res, 'launch-token', gateway)
    })
    const port = await listen(server)
    const headers = { 'x-dsh-token': 'launch-token', 'content-type': 'application/json' }
    const empty = await fetch(`http://127.0.0.1:${String(port)}${SHARE_CONTROL_PATH}`, {
      method: 'POST',
      headers,
      body: '',
    })
    expect(empty.status).toBe(200)
    const notObject = await fetch(`http://127.0.0.1:${String(port)}${SHARE_CONTROL_PATH}`, {
      method: 'POST',
      headers,
      body: 'null',
    })
    expect(notObject.status).toBe(400)
    const badEnabled = await fetch(`http://127.0.0.1:${String(port)}${SHARE_CONTROL_PATH}`, {
      method: 'POST',
      headers,
      body: JSON.stringify({ op: 'setNearby', enabled: 'yes' }),
    })
    expect(badEnabled.status).toBe(400)
    const noLan = await fetch(`http://127.0.0.1:${String(port)}${SHARE_CONTROL_PATH}`, {
      method: 'POST',
      headers,
      body: JSON.stringify({ op: 'setNearby', enabled: true }),
    })
    expect(noLan.status).toBe(409)
    expect(await noLan.json()).toMatchObject({ error: expect.stringContaining('no LAN') })
    const badAudience = await fetch(`http://127.0.0.1:${String(port)}${SHARE_CONTROL_PATH}`, {
      method: 'POST',
      headers,
      body: JSON.stringify({ op: 'setTailscaleAudience', audience: 1 }),
    })
    expect(badAudience.status).toBe(400)
    const ttl = await fetch(`http://127.0.0.1:${String(port)}${SHARE_CONTROL_PATH}`, {
      method: 'POST',
      headers,
      body: JSON.stringify({ op: 'openLoopback', ttlMs: 50 }),
    })
    expect(ttl.status).toBe(200)
    const shortToken = await fetch(`http://127.0.0.1:${String(port)}${SHARE_CONTROL_PATH}`, {
      method: 'POST',
      headers: { 'x-dsh-token': 'x', 'content-type': 'application/json' },
      body: JSON.stringify({ op: 'status' }),
    })
    expect(shortToken.status).toBe(401)
    const badHost = await rawRequest({
      port,
      path: SHARE_CONTROL_PATH,
      method: 'POST',
      headers: { host: 'example.test', 'x-dsh-token': 'launch-token' },
    })
    expect(badHost.status).toBe(401)
  })
})

function viWait(): Promise<void> {
  return new Promise(resolve => setTimeout(resolve, 50))
}
