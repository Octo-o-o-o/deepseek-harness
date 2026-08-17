# Agent Note: Desktop 分享页把其他浏览器接到正在跑的实例

Status: implemented

[English](2026-08-16-desktop-share-access.md) | 中文

## Problem

打包后的桌面应用已经在 WebView 里跑官方 Web GUI，但同一台机器的浏览器或其他设备打不开它。sidecar 只绑定回环，`apps/desktop/src-tauri/src/overlay.rs` 拒绝 `0.0.0.0` 和 `--trusted-host`，每次启动的 bootstrap nonce 又被 WebView 用掉。把 `http://127.0.0.1:<port>` 贴进 Chrome 会 401；同一 Wi-Fi 上的手机连 TCP 都完不成。用户想分享的控制面已经在了。缺的是给第二个 origin 一种可撤销的准入，以及在用户要求时接收附近或 tailnet 流量，同时不改变 `dsh web`。

## Decision

桌面壳一次交付完整的分享能力，不是做成 CLI 插件，也不是拆成多个产品版本。对第一稿的 Codex 审查（`proposals/2026-08-16-desktop-share-codex-review-raw.md`）证伪了「把启动 cookie 发给外部浏览器」、运行中 `--patch` 写入 trustedHosts、Tailscale `--bg` 随退出关闭、以及远程目录选择器自动降级；下面是实际落地的机制。操作路径见 [`proposals/2026-08-16-desktop-share-access.md`](../../../../proposals/2026-08-16-desktop-share-access.md)。

产品隐喻是「把这个窗口开到另一块屏幕」。菜单和托盘增加 **Open in Browser** / **在浏览器中打开** 与 **Use on Another Device…** / **在其他设备上使用…**。后者打开壳自己的原生小窗。外部浏览器永远拿不到启动 token。

sidecar 进程里有一个仅 Desktop 安装的 **share gateway**（只在成对的桌面环境变量存在时）：单独的 `http.Server`，持有可撤销的 `dsh-share` 会话，检查 Host/Origin/本机网卡，剥掉客户端 `X-DSH-Token` 和转发头，不转发 `/__dshd_*`，只在跳到回环 sidecar 时注入启动 token。本机浏览器流量改写成回环权威，设置面仍可用；附近和 Tailscale 流量改写成静态非回环名 `dshd.share.internal`，该名字在启动时就写在 desktop overlay 的 `trustedHosts` 里，因此 `PRIVILEGED_METHODS` 继续 403。`composeLive` 不重读 argv 里的 `--patch` overlay（`apps/cli/src/profile-boot.ts`），所以不用生成第二份 patch。

注入的 bootstrap 脚本在有 WebView nonce 时仍按今天的方式 POST；**没有 nonce 时不得 POST**，并且必须 resolve `__DSH_DESKTOP_BOOTSTRAP_DONE__`，否则已配对的浏览器会卡在连接循环里。在浏览器中打开走 `http://127.0.0.1:<gateway>/` 加短暂回环配对窗，ticket 不进 `open` 的 argv。扫码配对使用 prefix `/p`（webserver 没有参数路由）：GET 返回强制浅色 color-scheme 的 no-store 中间页。同源 POST 才消费票，并以 200 带 `Set-Cookie` 和 `location.replace('/')` 作答，不用 303，因为若干手机 WebView 会在重定向上丢掉 cookie。中间页用 `fetch` 提交，好让 Chrome 在跳转前把 cookie 存下来；`Referrer-Policy: no-referrer` 下的 form POST 会带 `Origin: null`，网关把它当成缺 Origin，同时仍拒绝 `sec-fetch-site: cross-site`。分享小窗跟随 `$DSH_HOME/settings.yaml` 里的 `ui-theme.preference`。

附近不改 sidecar 绑定。`overlay.rs` 继续 fail-closed。网关跟踪 HTTP、SSE 和 WebSocket，模式关掉或代次旋转时销毁它们。Tailscale Serve 保留外部 Host，且 `--bg` 比本应用活得长：壳对网关回环端口跑 **前台** `tailscale serve`，HTTPS 端口由本功能独占，绝不盲目 `off` 443。`setTailscaleAudience` 只在 `wait_https_listed` 成功之后才跑；失败会停掉 child 并把错误交回分享窗，因此绝不会给 Serve 尚未公布的端口发二维码。

`tapIndex` 仍注入 `randomUUID` polyfill。`desktop-state.json` 用读—校验—合并—改名写入 `{ nearby, tailscale }`。远程选文件夹被写成不可用——选择器后端看的是主监听的回环绑定，不是页面的 `isLoopback`。

没有桌面环境变量时以上都不装。`dsh web` 仍是回环且无认证。

这是在扩展 [每次启动的 token](2026-08-14-desktop-per-launch-token.md)，不改变 [CLI 的绑定地址](2026-07-22-web-bind-address.md)。分享窗放在壳里而不是 `sidebar.footer.action`，因为与 [插件重启](2026-08-16-desktop-plugin-restart-prompt.md) 不同：绑定状态属于壳。

## Alternatives considered

**做成 dsh-lan 那样的通用 `dsh` 插件。** overlay 把绑定改成 `0.0.0.0` 是官方组合接缝，polyfill tap 也是非安全上下文 UUID 的正确修法。但它仍然没有认证，装了它的每个 `dsh web` 用户都会把局域网 RCE 交出去，而且它画不了二维码、也驱不动 Tailscale。这条 fork 还禁止改变非桌面表层。

**把启动 token cookie 发给外部浏览器，并按客户端 Host 原样反代。** 第一稿。已被证伪：bootstrap 脚本会 POST 空 nonce 并卡住连接循环；cookie 不按端口隔离；关掉分享撤销不了 WebView 的 token；Tailscale 保留外部 Host 且 `--bg` 比进程活得长；`composeLive` 不重载 argv patch。

**附近打开时把 sidecar 改绑到 `0.0.0.0` 并重启。** 一次监听，复用 `resolveLanTrust`。否决原因是正在跑的 agent 会被杀掉。

**桌面组合始终绑定 `0.0.0.0`，靠准入 guard 挡。** 首次启动就会弹出操作系统防火墙，分享关闭时局域网仍能 SYN 到这个端口。

**把分享 UI 做进 sidecar 侧边栏。** 二维码必须留在对话旁边，而且新的 client 包会改这条 fork 希望从上游合并的 tsconfig 聚合文件。

**能力 URL 放在 fragment（`#dshd-nonce=`）。** 若干手机扫码器会丢掉 fragment。配对 URL 用 prefix `/p` 加中间页 POST。

**Cloudflare Tunnel 或 Tailscale Funnel。** 公网 HTTPS URL 是另一个产品。

**凡是带 token 的请求都解锁 `PRIVILEGED_METHODS`，或把远程 Host 改成回环。** 两种都会让手机去驱使 Mac 上的原生对话框。远程流量走 `dshd.share.internal`。

**手机上手输六位码。** 不当默认。

**`tailscale serve --bg`。** 官方说明会跨重启持续，直到显式 off，退出应用收不回端口。

## Testing

`pnpm vitest run packages/bundle/web-app --coverage.enabled --coverage.include='packages/bundle/web-app/src/**'`：64 个测试，包 src（含 `share-gateway.ts`）语句/分支/函数/行 100%。

`cd apps/desktop/src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`：clippy 干净，95 个测试通过。

CLI 姿势：两个桌面环境变量都不设时，`pnpm dsh --profile web --port 18765` 打印 `dsh web: http://127.0.0.1:18765`；`GET /p/x` 返回 HTTP 200、`text/html`，响应体含 `__DSH_BOOT__`，不含 `此码已失效` 或 `请回到 Desktop`。

## Consequences

「在浏览器中打开」和分享小窗能把另一块屏幕接到正在跑的实例，而不把启动 token 交给那块屏幕。附近和 Tailscale 默认关闭；关掉开关会旋转代次并销毁已跟踪的连接。用户原有的 443 Serve/Funnel 保持不动，因为本功能从不 `off` 那个端口。

拍下来的二维码是短时配对 URL。中间页 POST 和代次旋转限制这个窗口；文案仍须写明。

网关监听即使有分享会话也是新的攻击面。默认保持关闭。

局域网明文 HTTP 没有传输加密。窗口文案要写截获，不只写二维码被拍。

远程选文件夹、设置和凭据仍然不可用。假装现有 `isLoopback` UI 会降级它们是假的。

Wi-Fi 客户端隔离看起来会像产品缺陷。附近页要写出这种失败。
