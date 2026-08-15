# HTTP 服务器

[English](web-server.md) | 中文

[dsh-host-webserver](../../packages/host/webserver) 是 GUI 宿主的浏览器 HTTP 载体：它是一个提供 `ctx.webServer` 的 `node:http` 插件，包含具名路由注册表、index.html 转换回调，以及各一个可由插件认领的准入 guard 与回退处理器。它不属于 agent loop（智能体循环），也不是能力 seam；它不了解任何 harness 概念。其他插件负责注册所有功能路由，包括 `/api` 桥接、插件 bundle 和 HMR（热模块替换）事件流（[分层说明](../../.agents/notes/implemented/architecture/2026-07-19-gui-layering-and-rpc-protocol.md)）。该服务器只服务浏览器：Electron 通过 `file://` 加载已构建文件，并经 IPC 桥接发送 fetch 请求，不使用本服务器。

源码：[`packages/host/webserver/src/index.ts`](../../packages/host/webserver/src/index.ts)

## 路由

```ts type-equiv
/** Route match kind: 'exact' matches the pathname verbatim; 'prefix' p matches p and p/<anything>. */
type WebRouteKind = 'exact' | 'prefix'
```

```ts type-equiv
/** One named route registration. */
interface WebRoute {
  kind: WebRouteKind
  /** Absolute pathname, no trailing slash. */
  path: string
  /** Owns the full response lifecycle (may hold the response open, e.g. SSE). */
  handler: (req: IncomingMessage, res: ServerResponse) => void | Promise<void>
  /** Registration owner for desktop `/api` audit. */
  owner?: string
}
```

```ts type-equiv
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
type WebRequestGuard = (req: IncomingMessage, pathname: string) => number | undefined
```

匹配顺序固定：先过准入 guard，再查 exact 表，然后取最长匹配前缀，最后落到已注册的回退。guard 拒绝时以空响应体应答该状态码，在 upgrade 上则写出该状态行并关闭连接，而不交出 socket。注册顺序不携带任何面向请求的语义：具名路由在组合上互不相交，任何未被具名路由认领的请求都由回退席位应答；席位只有一个所有者，第二次注册会抛出异常。发布的 Web 组合用 [`dsh-host-frontend-static`](../../packages/host/frontend-static/src/index.ts) 认领席位，即遵循固定语义的 SPA dist 服务器：非 GET/HEAD 返回 405，越出 dist 根目录的遍历返回 403，任何未命中都以 HTTP 200 回退到 `index.html`（SPA 路由），未知扩展名按 octet-stream 发送。

## 配置

```ts type-equiv
/** Gateway config: the listen address. */
interface Config {
  /** Listen host; the two supported values are loopback and all-interfaces. */
  host: '127.0.0.1' | '0.0.0.0'
  /** Listen port; zero requests an OS-assigned port. */
  port: number
}
```

`host` 只接受 `127.0.0.1`（默认姿态）和 `0.0.0.0`（刻意的网络暴露）；该包自身不带 TLS、认证策略或 origin 策略——准入席位只运行其持有者装入的检查——因此绑定到非回环地址会把服务器暴露给该网络。dist 位置是认领席位的前端插件的组装事实。

## 服务

`WebServer`（`ctx.webServer`）在激活时立即监听；监听失败（EADDRINUSE 等）会使初始化被拒绝，启动进程会报告失败的 fiber。`register(route)` 添加一条具名路由并返回其 disposer；重复的 `(kind, path)` 抛出异常，因为路由模式是组合层约定，冲突即配置错误。`registerGuard(guard)` 认领唯一的准入席位，第二次注册会抛出异常，席位为空则全部放行；该席位存在的原因是路由只能为自己设防，因此一条必须同时覆盖「部署方无法控制的插件所注册的路由」的规则无处安放——桌面组合把它的启动 token 校验装在这里（[`dsh-web-app`](../../packages/bundle/web-app/README.md)）。`tapIndex(transform)` 添加一个纯 HTML 到 HTML 转换函数，按注册顺序应用于每个 index 响应（`/` 和每次 SPA 回退）；[dsh-client-modules](../../packages/client/modules) 用它注入启动 manifest（元数据清单）。`port` 读取监听端口，包括 `config.port` 为 0 时操作系统分配的端口。

处理过程中抛出异常的请求（畸形的 % 转义撞上 `decodeURIComponent`、客户端在请求体中途断开）会记录为警告并应答 400（响应头已发出时则销毁 socket），绝不导致进程退出。dispose（资源释放）把 `close()` 与 `closeAllConnections()` 配对使用，因为处理器可能像 SSE（Server-Sent Events）那样保持响应打开，而这类连接永远不会自行结束；没有强制关闭，拆卸就会挂起。该包从不打印输出：URL 行归 shell 所有。逐包运维细节（含开发模式的 bundle 监视流水线）留在 [README](../../packages/host/webserver/README.md) 中。

<!-- BEGIN GENERATED cordis-surface (gen-cordis-catalog.ts) — do not edit between markers -->

<a id="cordis-surface"></a>

## Cordis API

Generated from source by `scripts/gen-cordis-catalog.ts` (verified fresh by `pnpm run verify-cordis-catalog` in doc-sync; regenerate with `pnpm run gen-cordis-catalog`) — this section is byte-identical in both language sides of the page. Signature blocks use a `ts cordis-catalog` fence and keep the original source JSDoc; dispatch modes are defined in the [primer](../cordis-primer.md#dispatch-modes), and the framework-inherited `ctx` API lives in [cordis-api/inherited.md](../cordis-api/inherited.md).

<a id="ctxwebserver--webserver"></a>

### `ctx.webServer` — `WebServer`

The browser HTTP carrier service. Activation listens immediately. Route registration order does not affect requests because configured named routes must be distinct, and the fallback handler answers anything not yet claimed during startup with 404 until its owner registers. A listen failure rejects initialization, and the boot process reports the failed fiber.

```ts cordis-catalog
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
registerGuard(guard: WebRequestGuard): () => void

/**
 * Register a named route. Duplicate (kind, path) throws — route patterns are
 * a composition-level contract, so a collision is a misconfiguration.
 * @param route - kind, path, and the owning handler.
 * @returns the disposer removing the route.
 */
register(route: WebRoute): () => void

/**
 * Register an exact-path HTTP upgrade route. Duplicate paths throw because
 * one socket can have only one protocol owner.
 * @param route - pathname and handler owning negotiation plus socket use.
 * @returns the disposer removing the route.
 */
registerUpgrade(route: WebUpgradeRoute): () => void

/**
 * Claim the fallback seat: the handler answering every request no named
 * route matches (the SPA dist server in the shipped Web composition). One
 * owner only — a second registration throws, because two fallbacks cannot
 * compose.
 * @param handler - owns the full response lifecycle of unmatched requests.
 * @param owner - registration owner for composition audit.
 * @returns the disposer releasing the seat.
 */
registerFallback(handler: WebRoute['handler'], owner: string = 'unknown'): () => void

/**
 * Snapshot of named HTTP, upgrade, and fallback registrations.
 *
 * @returns a copy of each live registration's kind, path, and owner.
 */
listRegistrations(): WebRegistration[]

/**
 * Register an index.html transform, applied by the fallback owner to every
 * index response ({@link applyIndexTaps}) in registration order.
 * @param transform - pure html-to-html function.
 * @returns the disposer removing the transform.
 */
tapIndex(transform: (html: string) => string): () => void

/**
 * Run an index.html body through the registered taps in registration order
 * — called by the fallback owner on every index response it renders.
 * @param html - the raw index.html body.
 * @returns the transformed body.
 */
applyIndexTaps(html: string): string
```

Source: [`packages/host/webserver/src/index.ts:102`](../../packages/host/webserver/src/index.ts)
<!-- END GENERATED cordis-surface -->
