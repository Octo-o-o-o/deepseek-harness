# Agent Note：桌面 `/api` 鉴权改在 webserver 层，而非某一条 route 内部

Status: implemented

[English](2026-08-15-webserver-admission-guard.md) | 中文

## Problem

桌面模式在 connection 插件自己的处理器里校验本次启动的 token——`/api` 前缀 route 一次，每条 WebSocket upgrade 各一次。那是全部读取 token 的地方，而它只能给 connection 自己持有的 route 设防。

`WebServer.match()` 先解析 exact 表再看任何 prefix，因此注册 `exact /api/anything` 的插件会抢在 connection 的 `prefix /api` 之前应答，永远不经过 token 校验。桌面服务器绑定 loopback，而 loopback 不携带用户身份：任何本机进程——包括以另一个账户运行的进程——都能读到这类 route 提供的任何东西。

针对这一点的防线是 `assertDesktopApiExclusive`：只要有 `/api` 注册的 owner 不是 `client-connection` 就抛错。它在 Loader 结算之后运行，因此这个抛错会让整份组合失败。装了 [`dsh-usage-stats`](https://github.com/omdsh-dev/dsh-usage-stats)（在 `/api/usage-stats/` 下注册四条 exact route，提供 token 用量与 DeepSeek 账户余额）的用户，得到的是一个还没打印 ready 行就死掉的 sidecar，以及壳上一句 `sidecar stdout closed before the ready line`，真正的诊断只在 `$DSH_HOME/logs/sidecar.log` 里。

于是两个属性被反向耦合了：harness 为了保护一个本可以直接保护的命名空间而拒绝了一个正当的扩展点，而且是用「让应用起不来」的方式拒绝的。

## Decision

webserver 持有唯一的**准入席位（admission-guard seat）**。`registerGuard(guard)` 装入一个 `WebRequestGuard`，在查任何 route 表之前，对每个 HTTP 请求和每次 upgrade 运行；返回 `undefined` 表示放行，返回 HTTP 状态码表示拒绝。第二次注册会抛错，席位为空即全部放行，guard 抛出的异常与其他单请求失败一样被就地包容——HTTP 上回 400，upgrade 上销毁 socket。

该席位刻意保持通用：它接收 `(req, pathname)`、返回状态码，对 token、`/api` 或桌面一无所知。它之所以存在，是因为 route 只能给自己设防，因此一条必须同时覆盖「部署方无法控制的插件所注册的 route」的规则无处安放。

`dsh-web-app` 在自己的桌面分支里用 `desktopApiGuard(session)` 认领该席位：`/api` 之外放行，`/api` 之内要求本次启动的 token，走的是 connection 处理器所用的同一个 `hasValidDesktopToken`。因此插件自己的 `/api` 命名空间无需该插件实现任何东西即被鉴权；而不安装 guard 的 CLI 与浏览器表层，行为与此前完全一致。

`assertDesktopApiExclusive` 改为 `describeForeignApiRoutes`：一个返回诊断文本而非抛错的纯函数。第三方 `/api` route 不再是安全问题，但优先级仍然是问题——与某个 RPC 方法同名的 exact route 会在本次启动期间顶替该方法。报告列出每条 route 及其 owner，写到 stderr，桌面壳会把它 tee 进 `sidecar.log`。之所以不用 `ctx.logger.warn`：随附组合没有挂载任何 logger exporter，且 cordis 在默认级别下会抑制 `warn`，那个调用哪里也不会打印。

connection 插件保留它自己的两处 token 校验。它们是该插件配置面向 CLI 的行为，在没有安装 guard 的场合依然正确。

壳也不再只报告二次症状。ready 行失败现在会把 `sidecar.log` 的最后 20 行带进启动失败信息，因此任何致命加载失败——不只是这一种——都能到达错误页，而不是留在 `$DSH_HOME/logs` 里。

## Alternatives considered

**删掉断言，依赖信任栅栏。** `isTrustedApiRequest` 的 Host 栅栏同样住在 connection 的处理器里，因此它保护不了第三方 exact route 提供的任何东西。只删断言，等于把断言本来要阻止的未鉴权读取直接放出去。

**保留断言，但降级为「只在没安装 guard 时才抛」。** 桌面分支在该检查上方三行处无条件安装 guard，因此抛错那一支在构造上不可达——一道立在焊死敞开的门旁边的栅栏。

**让 guard 返回布尔值，把 401 硬编码在 webserver 里。** 那是把 harness 的授权决策放进一个「以不了解任何 harness 概念为要旨」的包。返回状态码则让策略留在其持有者身上。

**允许多个 guard。** 没有任何 Consumer 需要第二个，而两个席位也无法共同决定一个二元的准入。单席位与 `registerFallback` 对称，并沿用了它的 disposer 与抛错语义。

**匹配 connection 的 RPC 方法名，在注册时拒绝冲突。** connection 注册的是 prefix，因此除非 connection 主动告知，webserver 无从知道哪些子路径是方法——这种耦合会把 RPC 词汇塞进载体层。

## Verification

`pnpm vitest run packages/host/webserver packages/bundle/web-app packages/client/connection`——16 个文件、151 个测试，全部通过。

新增覆盖：webserver 套件通过真实 Loader 启动一份组合，断言被 guard 拦下的路径以 guard 给出的状态码拒绝且其已注册的处理器根本不运行、放行的请求命中的正是没有 guard 时会命中的 route、upgrade 两条路径同样成立、没有登记 reason phrase 的状态码仍产出可解析的状态行、抛出异常的 guard 被就地包容，以及席位确实只有一个（第二次注册抛错，disposer 恢复路由与可注册性）。web-app 套件断言桌面模式下席位被认领、无桌面鉴权时席位为空，以及第三方 `/api` route 产生的是 stderr 报告而不是失败的启动。

为证明这些测试能捕获缺陷而做的突变检查：把 `registerGuard` 改成不存储 guard、把 web-app 的 effect 改成不安装 guard 之后，恰好是那两个目标测试失败——

```
× runs the admission guard ahead of every route, on requests and on upgrades
× guards every /api path with the launch token, whoever registered the route
Test Files  2 failed | 3 passed (5)
      Tests  2 failed | 33 passed (35)
```

——撤销突变后两者重新通过。

`cargo fmt --check`、`cargo clippy --all-targets -- -D warnings` 与 `cargo test`（74 个通过）覆盖壳侧改动，包括越过字节窗口的日志尾部读取，以及日志可读与不可读两种情况下的启动失败信息。

在报告问题的机器上做端到端验证：profile patch 里重新启用 `dsh-usage-stats`，并设置两个桌面环境变量。此前会在 ready 行之前死掉的那份组合，现在先打印第三方 route 报告，再打印 `dsh web: http://127.0.0.1:45999`，且每条路径都符合设计：

| 请求 | 状态 |
|---|---|
| `GET /api/usage-stats/balance`，无凭据 | 401 |
| 同上，`X-DSH-Token` 正确 | 200，返回插件真实的余额载荷 |
| 同上，`X-DSH-Token` 错误 | 401 |
| `GET /api/usage-stats/usage`，带 `dsh-token` cookie | 200 |
| `POST /api/host.describe`，无凭据／带 token | 401／200 |
| upgrade `/api/events.mux`，无凭据 | `HTTP/1.1 401 Unauthorized` + `Connection: close` |
| 同上，带 `dsh-token` cookie | `HTTP/1.1 101 Switching Protocols` |
| `GET /__dshd_status`，带 `X-DSH-Bootstrap` | 200 |
| `GET /` | 200 |

## Consequences

`/api` 现在在载体层被鉴权一次，而不是由每个「记得做」的 route 持有者各鉴权一次。这比断言所保护的严格更强，因为它同样覆盖 connection 从未听说过的 route。

webserver 由此获得一个带安全含义的席位，其 README 与[子系统文档](../../../../docs/subsystems/web-server.md)现已写明：该包仍然不实现任何策略，但「授权无法在这里表达」已不再成立。

插件仍可用 exact `/api/<method>` route 顶替某个 RPC 方法。没有任何东西阻止它——那份报告只是诊断——而要阻止它，就得让载体层知道 connection 的方法词汇。
