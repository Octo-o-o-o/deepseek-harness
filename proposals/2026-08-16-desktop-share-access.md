# Desktop 多设备访问（一次交付 · Codex 修订终稿）

> 状态：**实施依据**。Codex `gpt-5.6-sol`（effort max，只读）已对照 `200fecbd69` 证伪原文若干机制；修订写入本文。原文审查：[2026-08-16-desktop-share-codex-review-raw.md](2026-08-16-desktop-share-codex-review-raw.md)。
> 决策记录：[`.agents/notes/implemented/feature/2026-08-16-desktop-share-access.md`](../.agents/notes/implemented/feature/2026-08-16-desktop-share-access.md)
> 不分产品版本。默认仍只听本机。

## 0. 一句话

用户用「在浏览器中打开」或扫码，把**同一个** Desktop 实例开到本机 Chrome、同一 Wi-Fi 的手机、或 Tailscale 上的其他设备。外部浏览器拿不到启动 token；壳上的分享网关验证独立的可撤销会话，转发时再注入 token。关掉开关会立刻拆掉网关连接。

## 1. 用户实际怎么用

产品隐喻是「把这个窗口开到另一块屏幕」。用户不需要知道端口、bind、token。

### 1.1 本机浏览器（约 5 秒）

1. 打开 dshd，和平时一样工作。
2. 菜单 **dshd → 在浏览器中打开**（托盘同名，⌘⇧B）。
3. 默认浏览器打开 `http://127.0.0.1:<gateway>/`（地址栏**没有** ticket）。壳在点击后的短窗口内接受来自回环的配对，避免把能力值放进 `open` 的 argv。
4. 与 Desktop 窗口同一份会话，可同时开着。本机 Chrome 视为这台电脑上的第二块屏幕，设置/选文件夹可用。

失败：浏览器看到中文说明「请回到 Desktop 再点一次」，不是 RPC 红字。

### 1.2 同一 Wi-Fi 的手机 / iPad（约 30 秒）

1. 菜单 **在其他设备上使用…**，独立小窗，不盖住聊天。
2. 「附近」页：大二维码 +「用手机相机扫码；微信请选在 Safari 中打开」。
3. 打开「允许附近设备访问」。窗底：*同一网络里完成扫码的人可以驱使这台电脑上的 agent。公共 Wi-Fi 请关掉。明文 HTTP 可被同网截获。*
4. 扫码打开 Web UI：能看对话、发消息、批准、停止。
5. **选文件夹、改设置、改密钥、在访达中打开只在这台电脑（含本机浏览器）上可用。** 手机上看这些入口会失败或不可用——主 sidecar 仍绑回环，目录选择器不会按页面 `isLoopback` 自动换成 browse。分享窗写明这一点；不为此去改共享 client 默认行为。
6. 关掉开关：已建立的 HTTP/SSE/WebSocket **立即断开**。再开需要重新扫码（旧 cookie 作废）。退出 dshd 后手机只看到浏览器连不上，没有「友好失效页」（进程已不在）。
7. 重启 dshd 后必须重新扫码。开关状态会记住。

多网卡：QR 默认 RFC1918 的 Wi-Fi 地址，可切换。网关还要校验请求打在哪张网卡上，不能只靠「QR 没画 Docker IP」来挡 `0.0.0.0`。

### 1.3 任何网络（Tailscale）

小窗第二页标题是 **「任何网络」**。

首次：未装 Tailscale 则说明并打开下载页。macOS 同时看 PATH 和 `/Applications/Tailscale.app/Contents/MacOS/Tailscale`（`TAILSCALE_BE_CLI=1`）。两边连上后点开启。dshd **前台**跑 `tailscale serve`（不用 `--bg`），目标是网关的回环端口，HTTPS 端口选本功能独占的、当前未被 Serve/Funnel 占用的端口。首次启用 HTTPS 若要浏览器同意，小窗里说明。二维码为 `https://<机器>.<tailnet>.ts.net/p/<ticket>`。

之后：若开关记得是开，启动时再拉起**我们自己的**前台 Serve。退出时关掉我们创建的那条，不碰用户原有的 443 Serve。

手机先打开 Tailscale 再扫码。书签在重启 dshd 后失效，小窗写明。

不做 Funnel / Cloudflare / ngrok。

### 1.4 停止

关开关或退出 dshd：销毁网关连接、停附近监听、停我们的 Serve 子进程。已发出的码和 cookie 立即作废。

### 1.5 失败文案

| 情况 | 桌面小窗 | 手机 |
|---|---|---|
| 访客 Wi-Fi 隔离 | 「连不上时用电脑热点，或改用任何网络」 | 浏览器自己的连接失败，不是我们的 HTML |
| 旧码 / 已关分享 | — | 若还能打到网关：失效页；若监听已关：连接失败 |
| Tailscale 未连 / 未装 | 「任何网络」页说明 | 打不开 HTTPS 名 |
| 已有别人的 Serve 占了端口 | 选别的端口或提示冲突，绝不 `--https=443 off` | — |

## 2. 体验上仍采用、机制上已改的决定

扫码 / 一键开浏览器仍是主路径。端口不进主文案。分享 UI 在壳，不在 Web 侧边栏。不重启 sidecar。`overlay.rs` 继续 fail-closed。

Codex 证伪后必须改的：外部浏览器不得持有启动 token；关开必须旋转会话代次并拆掉已有连接；Tailscale 指向网关且不用 `--bg`；「关掉后友好页」与「监听已关」不能两立；目录选择器不会自动降级。

## 3. 机制（share gateway）

```
本机 Chrome          附近手机 / Tailscale 手机
   │ loopback 配对窗      │ GET /p/<ticket> → 中间页 POST 消费
   ▼                     ▼
┌──────────────────────────────────────────────┐
│  Desktop share gateway（sidecar 进程内）      │
│  独立 dsh-share cookie，可按代次撤销          │
│  校验 Host / Origin / 本机网卡                │
│  剥掉客户端 X-DSH-Token 与 X-Forwarded-*      │
│  不转发 /__dshd_*                             │
│  转发时注入启动 token                         │
│  本机 Host→127.0.0.1；远程 Host→dshd.share.internal │
└──────────────────────────────────────────────┘
        ▼  主 sidecar 仍 --host 127.0.0.1
┌──────────────────────────────────────────────┐
│  官方 Web GUI。overlay.rs 不放宽。            │
│  trustedHosts 启动时含 dshd.share.internal    │
│  远程因此走非回环特权钉扎                     │
└──────────────────────────────────────────────┘
```

### 3.1 配对

- WebView 路径不变：fragment nonce → `dsh-token`。
- 注入脚本：**没有 nonce 时不得 POST bootstrap**，`__DSH_DESKTOP_BOOTSTRAP_DONE__` 直接 resolve。否则外部页会卡在 rejected Promise 上（`desktop-bootstrap.ts` 注入 + `connection.ts` await）。
- 外部浏览器只拿 `dsh-share`。网关转发时加 `X-DSH-Token`。
- **本机打开浏览器**：点击后开启数秒的回环配对窗，`open http://127.0.0.1:<gw>/`，ticket 不进 argv。
- **扫码**：`GET /p/<ticket>` 用 **prefix `/p`**（WebServer 没有参数路由）返回 `no-store`、`Referrer-Policy: no-referrer`、强制浅色 color-scheme 的内联页，同源 POST 才消费票、种 `dsh-share`、以 200 HTML 再 `location.replace('/')`（不用 303）。防止扫码器/微信预取消费。票 ≥128 bit、单次、短 TTL、绑精确 authority。
- 控制面 `POST /__dshd_share` 只接受 **header** token + 主 sidecar 的回环 Host，网关不转发。

### 3.2 网关

- 不是 `WebServer` 的第二 listen（dispatcher 是私有的）。在 Desktop 分支里自建 `http.Server`：反代 HTTP、SSE（`/plugins/events`）、WebSocket upgrade，跟踪连接，关闭时 `destroy`。
- 附近：在选定网卡/校验 local address 的前提下听外部端口。
- 换网后重建 audience 与 QR。
- 明文 LAN：cookie 无 `Secure`；Tailscale HTTPS：`Secure`。都是 HttpOnly。文案写明 LAN 可截获。

### 3.3 Tailscale

- Serve **保留**外部 Host（`machine.ts.net`），不会改成 loopback。禁止把远程伪装成回环来解锁特权。
- 远程在网关改写成 `dshd.share.internal`，该名字在 **sidecar 启动时** 就在 desktop overlay 的 `trustedHosts` 里。不写第二份 `--patch`：`composeLive` 只重读 profile/home 两个用户层，argv overlay 是启动快照（`profile-boot.ts`）。
- 不用 `--bg`。前台子进程，退出即停。启动前读现有 Serve/Funnel，占用冲突则换端口，永不盲目 `off` 443。
- macOS 探测含 App Store 二进制。

### 3.4 `randomUUID`

Desktop `tapIndex` polyfill。不改共享 client 调用点。

### 3.5 UI

壳的本地页 + 菜单/托盘。不新增侧边栏 client 包来做分享窗（避免再改 tsconfig 聚合——分享状态属于壳）。目录选择器的远程降级不假装已经存在。

### 3.6 `desktop-state.json`

读—校验—合并—临时文件—原子 rename。保留 `workspace` 与未知字段。损坏 JSON 不得静默覆盖。只在网关/Serve 真正进入目标状态后写 `{ nearby, tailscale }`。

## 4. Fork 红线

1. 无 `DSH_DESKTOP_*` 时 `dsh web` 不变：无网关、无 `/p`、无 polyfill、`--host 0.0.0.0` 仍拒绝。
2. 只改 Desktop 路径（`desktop-bootstrap.ts` / 其旁模块、`apps/desktop/**`、`desktop.patch.yml`）或缺席无行为的扩展点。
3. **禁止**改 `composeLive` watcher、CLI `startup.ts`、共享 WebServer 路由语法、`PRIVILEGED_METHODS`、默认 client。
4. `overlay.rs` 继续拒绝 sidecar argv 的 `0.0.0.0` 与 `--trusted-host`。

## 5. 安全

- 默认与现在相同。
- 外部持有的是可撤销 share 会话，不是启动 token。
- 关模式：generation++、清票、destroy 连接。
- 网关剥转发头、挡 `__dshd`、校验 Host/Origin/网卡。
- 分享窗 command 只授给壳窗口，不授给 sidecar origin。

## 6. 实施与门禁

一次做完：bootstrap 空 nonce no-op、网关、本机打开、附近 QR、Tailscale 前台 Serve、状态合并写、菜单小窗、CLI 姿势测试。

- `pnpm vitest run packages/bundle/web-app`
- `cd apps/desktop/src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`
- 无桌面 env 的 `dsh web`：`GET /p/x` 仍是今天的 SPA fallback 200，不是网关
- 网关集成：HTTP + SSE + WS 关闭即断
- 有 Tailscale 时：不破坏已有 443 Serve

## 7. 明确不做

通用 dsh 插件、自研 VPN、Funnel/Cloudflare/ngrok、独立手机 App、为 LAN 改共享 UUID 调用点、开分享时重启 sidecar、把启动 token 种给外部浏览器、`--bg` Serve、运行中热补 trustedHosts。
