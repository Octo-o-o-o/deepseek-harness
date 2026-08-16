## 总评（能否按该方案一次做完、是否该改 UX）

**当前方案不能按原文直接实施。** 一次发版仍然可行，但必须先重写配对、撤销和 Tailscale 三段；否则会同时出现：

- 配对成功后 Web UI 仍无法完成连接；
- 关闭再开启分享后，旧浏览器 cookie 继续有效；
- 运行中生成第二个 `--patch` 根本不会生效；
- `tailscale serve --bg` 可能在 dshd 退出后继续暴露端口；
- 手机端目录选择不会按宣称自动降级；
- “关掉分享后手机看到友好失效页”与“监听已关闭”互相矛盾。

建议保留“一键浏览器 + 附近二维码 + Tailscale”的产品外形，但把底层改成 **Desktop 专用 share gateway**：浏览器只持有可撤销的分享会话，gateway 验证外部 Host/Origin、拦截内部控制路由并向主 sidecar 注入 launch token。Tailscale 也应指向 gateway，而不是主 sidecar。

若坚持“完全不改任何 client UI”，则必须修改 UX 承诺：手机端“选文件夹”会显示但报错，不能称为“按现有 `isLoopback` 降级”。

## 对方案主张的逐条裁决（确认 / 部分成立 / 证伪）+ file:line

- **确认｜主 sidecar 可继续只绑定 loopback，且不需要重启。** `overlay.rs` 明确拒绝非 loopback 和 `--trusted-host`，[overlay.rs:35-59](/Users/wangyixiao/WorkSpace/Reference/deepseek-harness/apps/desktop/src-tauri/src/overlay.rs:35)；桌面启动固定使用 `--port 0 --host 127.0.0.1`，[sidecar.rs:73-94](/Users/wangyixiao/WorkSpace/Reference/deepseek-harness/apps/desktop/src-tauri/src/sidecar.rs:73)。主端口和拟议第二监听都使用 OS 分配端口，因此不存在 sidecar 端口冲突；真正的固定端口冲突在 Tailscale 443。

- **证伪｜`/p/<ticket>` 种 cookie 后 302 `/`，现有 bootstrap“不用改”即可工作。** 无 nonce 的页面仍会 POST 空 nonce 并产生 rejected promise，[desktop-bootstrap.ts:124-137](/Users/wangyixiao/WorkSpace/Reference/deepseek-harness/packages/bundle/web-app/src/desktop-bootstrap.ts:124)；服务端对此返回 400，[desktop-bootstrap.ts:175-183](/Users/wangyixiao/WorkSpace/Reference/deepseek-harness/packages/bundle/web-app/src/desktop-bootstrap.ts:175)。浏览器连接循环会等待该 promise，失败后进入 reconnect，[client/desktop-bootstrap.ts:16-20](/Users/wangyixiao/WorkSpace/Reference/deepseek-harness/packages/client/connection/src/client/desktop-bootstrap.ts:16)、[connection.ts:133-168](/Users/wangyixiao/WorkSpace/Reference/deepseek-harness/packages/client/connection/src/client/connection.ts:133)。所以方案 §3.1 的主路径当前必失败。必须把“无 nonce”改成 no-op，WebView 的单次 nonce 语义可以保持不变。

- **部分成立｜可以实现 `/p/<ticket>`，但不存在参数化 route。** WebServer 只有 `exact` 和 `prefix`，[webserver/index.ts:25-49](/Users/wangyixiao/WorkSpace/Reference/deepseek-harness/packages/host/webserver/src/index.ts:25)，匹配器也只做精确或最长前缀匹配，[webserver/index.ts:354-363](/Users/wangyixiao/WorkSpace/Reference/deepseek-harness/packages/host/webserver/src/index.ts:354)。应注册 `/p` prefix 后自行严格验证“仅一个 base64url path segment”，不要为 Desktop 修改共享路由器。当前未注册的 `GET /p/x` 会被 SPA fallback 以 200 index 回答，并非 404，[frontend-static/index.ts:69-85](/Users/wangyixiao/WorkSpace/Reference/deepseek-harness/packages/host/frontend-static/src/index.ts:69)。

- **证伪｜复用 WebView 的 `dsh-token` cookie 能满足关闭即撤销。** 当前 cookie 是 `Path=/`、无 Max-Age 的 launch-wide token，[desktop-bootstrap.ts:184-195](/Users/wangyixiao/WorkSpace/Reference/deepseek-harness/packages/bundle/web-app/src/desktop-bootstrap.ts:184)。关闭监听不会使该 cookie 或 token 失效；同一次 dshd 运行中再次打开分享，旧浏览器会立即重新获得权限。cookie 也不按端口隔离，因此把同一个 launch token 放进普通浏览器后，127.0.0.1 上其他端口的服务可能收到它；这是 [RFC 6265 明确指出的弱隔离](https://datatracker.ietf.org/doc/html/rfc6265#section-8.5)。必须使用独立、服务端可撤销的 share-session cookie，不能把 launch token交给外部浏览器。

- **部分成立｜第二个 HTTP+WS proxy 在 `desktop-bootstrap.ts` 中技术上可做，但不是 harness 已支持的“第二 listen”。** `WebServer` 的底层 `Server` 和 dispatcher 都是私有的，只公开 host/port/注册接口，[webserver/index.ts:102-131](/Users/wangyixiao/WorkSpace/Reference/deepseek-harness/packages/host/webserver/src/index.ts:102)。需要另建完整 HTTP server、反向 HTTP client、Upgrade tunnel、连接跟踪和 teardown。还要流式转发 SSE；`/plugins/events` 是长期 SSE，[client/hmr/index.ts:148-187](/Users/wangyixiao/WorkSpace/Reference/deepseek-harness/packages/client/hmr/src/index.ts:148)，不能只覆盖普通请求和 WebSocket。

- **部分成立｜LAN proxy 保留 Host 是正确方向，但静态 trusted-host snapshot 不支持运行期换网。** Host 合法后，存在 `Origin` 时还必须与 Host authority 完全相同，[api-request-trust.ts:103-129](/Users/wangyixiao/WorkSpace/Reference/deepseek-harness/packages/client/connection/src/api-request-trust.ts:103)。现有 `resolveLanTrust` 只有主 WebServer 绑定 `0.0.0.0` 才采地址，[web-app/index.ts:88-105](/Users/wangyixiao/WorkSpace/Reference/deepseek-harness/packages/bundle/web-app/src/index.ts:88)；Desktop 主监听仍是 loopback，因此不会自动得到 LAN trust。方案里的自定义 `!!js` snapshot 可以在启动时做，但 DHCP、Wi-Fi 切换、睡眠恢复后会过期。

- **证伪｜运行中写第二个 `--patch`，`composeLive` 会重读它。** `--patch` 的确可重复，[args.ts:57-61](/Users/wangyixiao/WorkSpace/Reference/deepseek-harness/apps/cli/src/args.ts:57)，但 argv overlays 只在启动时读取，[profile-boot.ts:142-170](/Users/wangyixiao/WorkSpace/Reference/deepseek-harness/apps/cli/src/profile-boot.ts:142)。`composeLive` 只重新读取 profile 与 home 两个固定 patch，并复用冻结的 `composed.overlays`，[profile-boot.ts:227-245](/Users/wangyixiao/WorkSpace/Reference/deepseek-harness/apps/cli/src/profile-boot.ts:227)；watcher 也只监听这两个文件，[profile-boot.ts:285-294](/Users/wangyixiao/WorkSpace/Reference/deepseek-harness/apps/cli/src/profile-boot.ts:285)。Desktop 当前只在 spawn 前传一个 patch，[lib.rs:276-285](/Users/wangyixiao/WorkSpace/Reference/deepseek-harness/apps/desktop/src-tauri/src/lib.rs:276)。不能通过修改 `apps/cli` 去“补救”，那会改变全部 CLI 表层。

- **证伪｜Tailscale 可能把 Host 改成 loopback，届时只需接受远程特权解锁。** 当前 Tailscale Serve 的实现对 TCP 后端明确保留 incoming Host，[serve.go:3851-3888](https://github.com/tailscale/tailscale/blob/main/ipn/ipnlocal/serve.go#L3851-L3888)，因此实际 Host 是 `machine.tailnet.ts.net`。若某代理只改 Host、不改 Origin，本仓会因二者不一致而 403；若两者都伪装为 loopback，又会让空 trust-list 的特权检查通过。方案 §3.3 的“接受解锁”分支应删除，而不是保留为兼容路径。

- **确认｜在保持外部非 loopback authority 时，现有特权方法会继续拒绝手机。** `host.pickDirectory`、`openPath`、settings、credentials 等都在固定列表内，[connection/index.ts:97-127](/Users/wangyixiao/WorkSpace/Reference/deepseek-harness/packages/client/connection/src/index.ts:97)，并以空 trust-list 二次校验，[connection/index.ts:150-160](/Users/wangyixiao/WorkSpace/Reference/deepseek-harness/packages/client/connection/src/index.ts:150)。但这只覆盖 Typert RPC，不等于整个 WebServer 的所有插件路由都有远程权限分类。

- **部分成立｜透明反代并不会自动让整个 sidecar 获得 token 保护。** Desktop 的 server-wide guard 只保护 `/api`，[desktop-bootstrap.ts:284-305](/Users/wangyixiao/WorkSpace/Reference/deepseek-harness/packages/bundle/web-app/src/desktop-bootstrap.ts:284)。connection 自己的顶层 RPC channel 会验 token，[rpc-host.ts:95-124](/Users/wangyixiao/WorkSpace/Reference/deepseek-harness/packages/client/connection/src/rpc-host.ts:95)，但其他插件 route 没有这一保证；例如 `/plugins/events` 本身不鉴权。share gateway 必须在边缘验证每个转发请求，并屏蔽 `__dshd` 控制路由，不能把“浏览器有 cookie”当成 WebServer 全局认证。

- **证伪｜`tailscale serve --bg` 符合“退出 dshd 即关闭”。** 官方文档说明 `--bg` 会跨设备/Tailscale 重启持续恢复，直到显式 off；关闭时还要求重复原 flags。[Tailscale Serve CLI](https://tailscale.com/docs/reference/tailscale-cli/serve#effects-of-rebooting-and-restarting)。此外首次 Serve 可能打开网页登录页以启用 HTTPS，Serve 与 Funnel 不能共用同一端口，[Tailscale Serve](https://tailscale.com/docs/features/tailscale-serve)。方案还未处理已有用户 Serve 配置，直接占用或 off 443 可能破坏别的服务。

- **部分成立｜PATH 探测不足以判断 macOS 已安装 Tailscale。** App Store 版 CLI 位于 `/Applications/Tailscale.app/Contents/MacOS/Tailscale`，Standalone 版只有安装 CLI integration 后才进入 `/usr/local/bin`；脚本执行 app binary 还应设置 `TAILSCALE_BE_CLI=1`。[Tailscale CLI 官方说明](https://tailscale.com/docs/reference/tailscale-cli?tab=macos)。否则产品会把“已安装”误报成“未安装”。

- **确认｜`tapIndex` polyfill 是正确的 Desktop-only 修复面。** 注入发生在 `<head>` 最前且在模块脚本前，[desktop-bootstrap.ts:130-141](/Users/wangyixiao/WorkSpace/Reference/deepseek-harness/packages/bundle/web-app/src/desktop-bootstrap.ts:130)、[web-app/index.ts:173-175](/Users/wangyixiao/WorkSpace/Reference/deepseek-harness/packages/bundle/web-app/src/index.ts:173)。它可覆盖 WebApiClient 的直接调用，[apiproxy/fetch/client.ts:292-301](/Users/wangyixiao/WorkSpace/Reference/deepseek-harness/packages/host/apiproxy/src/fetch/client.ts:292)，以及附件 draft，[ui-conversation/service.ts:61-68](/Users/wangyixiao/WorkSpace/Reference/deepseek-harness/packages/client/ui-conversation/src/client/service.ts:61)。规范也确实把 `randomUUID` 标为 SecureContext、而 `getRandomValues` 没有该限制，[Web Crypto](https://www.w3.org/TR/webcrypto/#Crypto-method-randomUUID)。

- **证伪｜手机 UI 会按现有 `isLoopback` 自动降级目录选择。** 客户端能正确算出远程 `isLoopback=false`，[client/index.ts:87-110](/Users/wangyixiao/WorkSpace/Reference/deepseek-harness/packages/client/connection/src/client/index.ts:87)，但 directory-picker 在启动时只看主 WebServer bind；Desktop 主监听是 loopback，所以 macOS 会选择 native backend，[directory-picker-auto/index.ts:62-68](/Users/wangyixiao/WorkSpace/Reference/deepseek-harness/packages/host/directory-picker-auto/src/index.ts:62)、[resolve.ts:47-52](/Users/wangyixiao/WorkSpace/Reference/deepseek-harness/packages/host/directory-picker-auto/src/resolve.ts:47)。客户端仍注册 native flow 并调用 `host.pickDirectory`，[ui-directory-picker-native/index.ts:26-39](/Users/wangyixiao/WorkSpace/Reference/deepseek-harness/packages/client/ui-directory-picker-native/src/client/index.ts:26)、[workspaces/service.ts:205-214](/Users/wangyixiao/WorkSpace/Reference/deepseek-harness/packages/client/runtime/src/client/workspaces/service.ts:205)，最终在手机上显示错误。

- **确认｜`desktop-state.json` 当前不能保存可变开关。** `ensure_desktop_state` 在文件存在时直接返回，[paths.rs:88-111](/Users/wangyixiao/WorkSpace/Reference/deepseek-harness/apps/desktop/src-tauri/src/paths.rs:88)，测试也固定了只写一次行为，[paths.rs:364-373](/Users/wangyixiao/WorkSpace/Reference/deepseek-harness/apps/desktop/src-tauri/src/paths.rs:364)。方案识别到了缺口，但“合并写”还需原子替换、保留未知字段和损坏 JSON 策略。

- **部分成立｜分享小窗 capability 分离是正确的，但 shell 尚无控制状态。** 当前 `AppState` 不保存 sidecar port/token，[lib.rs:61-70](/Users/wangyixiao/WorkSpace/Reference/deepseek-harness/apps/desktop/src-tauri/src/lib.rs:61)；它们只存在于启动线程局部变量，[lib.rs:255-263](/Users/wangyixiao/WorkSpace/Reference/deepseek-harness/apps/desktop/src-tauri/src/lib.rs:255)。现有动态 capability 仅给主窗口 sidecar origin 两个插件重启命令，[lib.rs:387-410](/Users/wangyixiao/WorkSpace/Reference/deepseek-harness/apps/desktop/src-tauri/src/lib.rs:387)。新窗口应有独立 label/capability，且必须新增可并发访问的 share control state。

- **部分成立｜fork 红线总体正确，但几条拟议“补救”越界。** 修改 Desktop 条件分支、`apps/desktop/**` 和 `desktop.patch.yml` 合规；修改 `composeLive`、允许 CLI `0.0.0.0`、给 WebServer 增加参数路由、或改变 `PRIVILEGED_METHODS` 都不合规。当前 CLI 明确拒绝 `0.0.0.0`，[startup.ts:99-106](/Users/wangyixiao/WorkSpace/Reference/deepseek-harness/packages/bundle/web-app/src/startup.ts:99)。因此 API trust Agent Note 中“CLI 已支持 0.0.0.0”的描述已落后于代码，[api-browser-trust-boundary.md:9-29](/Users/wangyixiao/WorkSpace/Reference/deepseek-harness/.agents/notes/implemented/architecture/2026-07-28-api-browser-trust-boundary.md:9)。

## 方案未写但会让产品失败的缺陷（P0/P1/P2）

### P0

- **没有可撤销的分享会话。** 同一 launch token 同时服务 WebView、本机浏览器、LAN 和 Tailscale；关开分享不能撤销旧 cookie，LAN 泄漏还会扩大到其他模式。
- **关闭监听不等于关闭已有连接。** Node 的 `server.close()` 不会自动终止 upgraded sockets；现有 WebServer 为此显式追踪并 destroy WebSocket，[webserver/index.ts:286-350](/Users/wangyixiao/WorkSpace/Reference/deepseek-harness/packages/host/webserver/src/index.ts:286)。第二 proxy 必须做同样的事情，否则“关掉即收回”不成立。
- **配对控制路由边界未定义。** “loopback + token”若用 socket 地址判断，在 Tailscale/LAN 反代后上游 peer 仍是 loopback；若复用 `hasValidDesktopToken`，它同时接受 cookie，[api-request-trust.ts:132-159](/Users/wangyixiao/WorkSpace/Reference/deepseek-harness/packages/client/connection/src/api-request-trust.ts:132)。已配对远程浏览器可能再次签票。控制 API 必须只接受 header token、精确主 authority，并且不能由 share gateway 转发。
- **Tailscale 运行中 hostname 注入路径不存在。** 当前 Host 又确定是 ts.net；原方案中途开启远程的关键路径无法工作。
- **Tailscale 443 外部状态可能被覆盖。** 必须先读取现有 Serve/Funnel 配置、选择本功能独占的 HTTPS 端口并只清理自己创建的配置，不能盲目 `--https=443 off`。

### P1

- **ticket GET 会被二维码扫描器、微信安全预览或消息预览提前消费。** 应由 GET 返回不消费 ticket 的无外链页面，再由同源 JS POST 原子消费并 `location.replace('/')`。
- **`SameSite=Strict + 直接 302` 不应被当成跨浏览器恒定行为。** Strict cookie 不随由跨站文档触发的顶层导航发送，[最新 6265bis 草案](https://datatracker.ietf.org/doc/html/draft-ietf-httpbis-rfc6265bis#name-strict-and-lax-enforcement)。相机启动、Safari、微信 WebView 的上下文差异必须真机验证；同源中间页可避开这一依赖。
- **LAN HTTP 的 ticket 和 cookie 都是明文。** `HttpOnly` 只阻止 JS 读取，不提供链路加密；`Secure` cookie 又不能用于 HTTP LAN。[RFC 6265 Secure/HttpOnly 语义](https://datatracker.ietf.org/doc/html/rfc6265#section-4.1.2.5)。Tailscale HTTPS cookie 必须加 `Secure`，LAN 文案需明确“同网可截获”而不仅是“拿到二维码”。
- **“关掉/退出后手机看到友好失效页”不可能与“监听/进程已关闭”同时成立。** 没有服务器就只能看到浏览器连接失败。应修改 §1.5 文案；不要为了友好页保留 tombstone listener。
- **`0.0.0.0` 暴露的是全部接口，不是“附近 Wi-Fi”。** UI 不把 Tailscale/Docker 地址编进 QR，并不会阻止监听接受这些接口的连接。应按选定接口绑定，或在 gateway 同时检查 socket local address 与精确 Host。
- **IP snapshot 会随换网失效。** Wi-Fi/Ethernet 切换、DHCP、睡眠恢复后，QR、ticket audience 和 trust 都需重建；当前无网络变更监听。
- **本机 `open` 会把完整 ticket URL放进 launcher argv。** [opener.rs:26-39](/Users/wangyixiao/WorkSpace/Reference/deepseek-harness/apps/desktop/src-tauri/src/opener.rs:26)。ticket 虽短时单次，仍是能力值；方案的日志威胁模型必须覆盖进程参数、浏览器历史、Tailscale/stdout 和 proxy 错误，而不只是 `sidecar.log`。
- **WeChat 不能未经真机验证就写成支持。** 至少提供“在 Safari 中打开 / 复制链接”fallback；微信可能预取链接、限制私网 HTTP 或使用独立 cookie jar。
- **Tailscale WS 需要长时间实测。** 当前实现使用 Go reverse proxy，但已有一条针对 1.94.2 的未决断连报告；这不是所有版本必现的证据，却足以要求 30 分钟以上 soak test：[tailscale/tailscale#18827](https://github.com/tailscale/tailscale/issues/18827)。

### P2

- Polyfill 只补 `randomUUID`；plain HTTP 下 Clipboard API 仍可能缺失。大部分复制按钮有 fallback，但 JsonTree 直接调用 `navigator.clipboard` 并显示失败，[JsonTree.tsx:530-538](/Users/wangyixiao/WorkSpace/Reference/deepseek-harness/packages/client/ui-primitives/src/JsonTree.tsx:530)。
- “防火墙提示同步文案”没有可观察信号。macOS 可能自动允许签名程序，也可能按用户策略阻止；只能显示静态指导或连接超时诊断。[Apple macOS firewall](https://support.apple.com/guide/security/firewall-security-in-macos-seca0e83763f/web)。另外 Apple 明确说明“监听/接受 incoming TCP”本身不要求 Local Network 权限，Safari/WKWebView 流量也例外，[TN3179](https://developer.apple.com/documentation/technotes/tn3179-understanding-local-network-privacy)。
- 当前设计只描述 IPv4；应明确 IPv6 不支持，而不是让多网卡 UI 暗示全覆盖。
- 被引用的 per-launch-token Note 已落后于代码：Note 写 30 秒和 `Path=/api`，[note:13](/Users/wangyixiao/WorkSpace/Reference/deepseek-harness/.agents/notes/implemented/feature/2026-08-14-desktop-per-launch-token.md:13)，实际是 120 秒和 `Path=/`。修订方案时应同步纠正权威记录。

## 必须改的方案修正（具体改哪一段）

1. **重写 §3 总图与 §3.1。**

 改为：外部浏览器 → Desktop share gateway → 主 sidecar。gateway 发独立 `dsh-share` 会话，服务端保存 `{mode, generation, audience, expiresAt}`；浏览器永远拿不到 launch token，gateway 转发时再注入。关开模式时旋转 generation、清 ticket/session、销毁 HTTP/WS 连接。

2. **修改 bootstrap 脚本。**

 有有效 nonce 时保持现有 WebView 单次交换；无 nonce 时不发 bootstrap POST，让已由 gateway 配对的浏览器直接连接。未经配对的 `/` 应由 gateway 返回友好页，而不是把 SPA送下去后再 401/reconnect。

3. **把 ticket 流程改成两步但保持“一次扫码”。**

 `GET /p/<ticket>` 只返回 `no-store`、`Referrer-Policy: no-referrer` 的内联页面；页面同源 POST 原子消费 ticket、设置 share cookie，再 `location.replace('/')`。ticket 至少 128-bit、单次、短 TTL、每模式只保留一个当前票，并绑定 exact external authority。

4. **重写 §3.2 为真正的安全 gateway。**

 gateway 必须：

   - 对外部 Host、Origin、local interface 做精确校验；
   - 丢弃客户端提供的 `X-DSH-Token`、`Forwarded`、`X-Forwarded-*` 和 hop-by-hop headers；
   - 屏蔽 `/__dshd_*` 控制路由；
   - 对所有转发请求和 Upgrade 先验 share session；
   - 支持流式 HTTP/SSE、上传 backpressure、WebSocket head bytes；
   - 跟踪并在 off 时销毁普通连接和 upgrade sockets。

   远程模式转发到主 sidecar 时，将 Host/Origin 改成一个预先配置的、**非 loopback** Desktop 专用 authority；gateway 必须在改写前验证真实外部 authority。这样无需动态 LAN/Tailscale hostname patch，同时 `PRIVILEGED_METHODS` 继续得到 403。本机浏览器可以保留 loopback authority，从而拥有本机能力。

5. **彻底替换 §3.3。**

 Tailscale 指向 gateway 的 loopback listener，不指向主 sidecar；删除“Host 为 loopback时接受远程特权”和“运行中生成第二 patch”两条。优先持有前台 `tailscale serve` 子进程，不使用 `--bg`；使用功能独占的 HTTPS 端口，启动前读取现有 config/status，首次 HTTPS consent 显式呈现在小窗里。macOS 探测同时覆盖 PATH 与 App Store app binary。

6. **修改 §1.2/§3.5 的远程降级承诺。**

 两个可选方案必须明确选一个：

   - 允许增加一个缺席时无行为变化的 Desktop-only client seam，在 `isLoopback=false` 时隐藏/替换 native directory flow；或
   - 明确手机端该控件暂不可用，并接受这不是完整降级。

   当前“不新增 client UI 且现有 UI 自动降级”不可同时成立。

7. **补全 §3.6。**

 `desktop-state.json` 更新采用读—校验—合并—临时文件—原子 rename；保留 `workspace` 和未知字段，损坏 JSON 不得静默覆盖。只在 gateway/Serve 确实进入目标状态后更新开关。

8. **强化 §4 fork 红线。**

 明写：不得为本功能修改 `composeLive` watcher、CLI `startup.ts`、共享 WebServer route 语法或默认 client 行为。若必须增加 client seam，默认配置必须逐字保持当前行为，只有 Desktop patch 启用。

## 可以保留的决定

- 默认只监听 `127.0.0.1`，分享显式开启。
- 不重启正在运行 agent 的 sidecar。
- `overlay.rs` 保持 fail-closed。
- 分享 UI、二维码和状态归 Tauri 壳所有。
- 不做 Funnel、Cloudflare Tunnel、ngrok 或自研 VPN。
- QR 使用 path ticket，而不是要求扫描器保留原始 fragment。
- `tapIndex` 注入 `randomUUID` polyfill。
- 远程 Host 必须保持非 loopback 权限语义。
- 分享小窗的 Tauri commands 与 sidecar origin capability 分离。
- 一次发布完整 UX；这里要求的是实施前修正设计，不是拆成多个产品版本。

## 实施时的验证清单

- **配对与撤销**

  - 本机浏览器首次打开、刷新、第二标签页均能连接，且不会 POST 空 nonce。
  - ticket 并发消费只有一个成功；过期、重放、扫描器预取均显示正确结果。
  - nearby/tailscale off 时现有 HTTP、SSE、两条 WebSocket 立即断开。
  - 再次开启后旧 cookie、旧 ticket 和书签均不能重新获得权限。
  - dshd 退出时接受浏览器网络错误，不伪测“友好失效页”。

- **Host/Origin/权限矩阵**

  - 合法 LAN IP、合法 ts.net、Host/Origin mismatch、恶意 Host、DNS-rebind Host、`Sec-Fetch-Site: cross-site`。
  - 外部伪造 `X-DSH-Token` 和 forwarding headers 被剥离。
  - 手机可 `session.prompt`、批准、停止；settings/credentials/pickDirectory/openPath 均为 403 或控件不可见。
  - 本机 Chrome 是否应拥有特权方法必须单独定案并测试。
  - 任意非 `/api` 插件 route 也不能绕过 share-session gateway。

- **浏览器与网络**

  - 真机 iOS Safari、微信 WebView、Android Chrome；相机扫码与“在 Safari 中打开”。
  - LAN HTTP 下 polyfill、附件、复制按钮、SSE 和长时间 WS。
  - Wi-Fi/Ethernet 同时存在、DHCP 变更、睡眠恢复、热点、访客网隔离、Tailscale/Docker/169.254 接口。
  - 已签名打包应用在 macOS firewall 开、关、Block All、自动允许签名软件四种姿势下验证。

- **Tailscale**

  - 未安装、App Store 安装但 PATH 无 CLI、未登录、HTTPS 未启用、ACL 拒绝。
  - 已有 Serve/Funnel 443 配置必须原样保留。
  - 当前安装版本实测 Host/Origin、HTTP、SSE、WebSocket，并至少做 30 分钟 WS soak。
  - 强杀 dshd、强杀 Tailscale、系统重启后都不存在遗留后台 Serve；保存的开关只负责下一次重新启动本功能拥有的前台 Serve。

- **cookie 与日志**

  - LAN cookie 无 `Secure`、Tailscale cookie有 `Secure`；两者都 `HttpOnly`、适当 SameSite、独立于 launch token。
  - 抓包确认 LAN 链路只暴露可撤销 share credential，绝不暴露 launch token。
  - 检查 `ps`、sidecar/shell 日志、Tailscale stdout/stderr、proxy 错误、浏览器历史，完整 ticket/token 均不得落盘；若 opener argv 中保留 ticket，必须作为明确接受的剩余风险记录。

- **状态与 fork**

  - 状态写入保留 workspace/未知字段；损坏 JSON、并发切换和中途崩溃不会截断文件。
  - 无两个 Desktop env 时：无 pair/control 路由、无 polyfill、无第二监听、无 share authority。
  - 实跑无 env 的 `pnpm dsh --profile web --port <p>`；`/`、`/api`、`/p/x` 保持当前行为，`--host 0.0.0.0` 仍按当前代码拒绝。
  - focused Vitest、真实 HTTP+SSE+WS proxy 集成测试，以及提案列出的 Cargo fmt/clippy/test。

本次为只读评审：已核对 HEAD `200fecbd6960523d5946b9188b44418935bc6bb9`；未修改文件，`git diff --stat` 与 `git diff --name-only` 均为空。未运行产品测试，以上运行期结论明确列入了实施验证项。