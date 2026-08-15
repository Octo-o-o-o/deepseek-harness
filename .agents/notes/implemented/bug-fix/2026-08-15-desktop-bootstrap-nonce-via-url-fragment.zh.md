# Agent Note: 桌面 bootstrap nonce 改经 URL fragment 送达

Status: implemented

[English](2026-08-15-desktop-bootstrap-nonce-via-url-fragment.md) | 中文

## Problem

桌面壳此前把一次性 bootstrap nonce 写进所提供的 index 交给页面：`injectDesktopBootstrapScript(html, nonce)` 产出 `window.__DSH_DESKTOP_BOOTSTRAP__ = "<nonce>"`，页面再把该值 POST 给 `/__dshd_bootstrap`，换取 `/api` 用的 HttpOnly `dsh-token` cookie。

于是 nonce 就位于一条无需鉴权的路由所返回的响应体中。对运行中的实例实测：`GET /` 无任何凭据即返回 200，因此任何能访问该 loopback 端口的本机进程都能读到 nonce、换取 token cookie 并驱动 `/api`。loopback 不携带用户身份，所以这不限于用户自己的进程，**本机其他账户**的进程同样可以。而 agent 提供 bash 工具，这条路径等价于本地提权至任意代码执行。

`/api` 命名空间本身不是弱点：它对无凭据请求正确返回 401。公开的只是 nonce 的送达通道。

## Decision

壳导航到 `http://127.0.0.1:<port>/#dshd-nonce=<nonce>`，注入脚本改从 `location.hash` 读取。user agent 从不把 fragment 发上网络，因此 nonce 无需出现在任何响应体即可送达页面，所提供的 index 也在各次启动之间逐字节相同。

`injectDesktopBootstrapScript(html)` 不再接收 nonce。`encodeBootstrapNonceLiteral` 一并删除：它的唯一用途是转义嵌入 HTML 的 nonce，而这件事已不再发生。

脚本读取后调用 `history.replaceState` 移除该键，并保留 fragment 中其余无关状态。这样 nonce 既不留在前进/后退条目里，也不会被之后运行的页面脚本读到。fragment 缺失时得到空 nonce，`/__dshd_bootstrap` 会拒绝——未经壳打开的页面拿不到 cookie，而不是进入半成品会话。

`navigate_to_sidecar` 接收 nonce 并构造该 URL。nonce 来自 `generate_desktop_token`，为十六进制，fragment 无需百分号编码。`is_internal_url` 与 `is_sidecar_origin` 比较的是 scheme、host 与 port，新增 fragment 不影响导航围栏；读取后的 `replaceState` 也使 origin 检查继续通过。

## Alternatives considered

**把 loopback 监听绑定到用户。** 原理上正确，但 loopback TCP 在 macOS 与 Windows 上没有可移植的对端 UID 校验，且这并不能把密钥从公开响应体中拿掉。

**要求 `GET /` 携带 `X-DSH-Bootstrap` 头。** 首次导航由 WebView 发起而非壳的 HTTP 客户端，壳无法为其附加请求头。

**进一步缩短 nonce TTL。** 该时长覆盖整个启动预算是有原因的：实时杀毒扫描下的首次启动可能超过 15s。缩短它是拿真实的启动可靠性问题去换一个仍然存在、只是更小的竞态窗口。

## Verification

`packages/bundle/web-app/tests/desktop-bootstrap.spec.ts` 在 `vm` 上下文中以打桩的 `location`/`history` 对注入脚本求值：nonce 取自 fragment；无关 fragment 状态在该键被移除后仍保留；fragment 缺失时得到空 nonce 且不改写 history；敌意 fragment 内容保持为不透明值而不会被执行。其中一例断言两次不带 nonce 参数的注入产出完全相同的 HTML，从而钉住「所提供的字节不含密钥」这一性质。

`apps/desktop/src-tauri` 下 `cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`、`cargo test` 全部通过；web-app 包测试 18 项通过；重新记录桌面 README 配对后，`verify-translation-pairing` 报告 956 对一致。

尚未覆盖：以打包应用完整跑通 fragment 路径的 WebView 启动。下一个签名构建必须在发布前确认，因为单测打桩的是 `location`，而非真实的 WebView 导航。

## Consequences

所提供的 index 不再随启动密钥变化，攻击者即便触达 loopback 端口，也无法从 `GET /` 获得任何东西。剩余暴露面是导航 URL 本身，而它停留在 WebView 进程内部。

任何绕过壳直接打开 sidecar URL 的方式——例如为调试用浏览器指向该端口——现在会得到一个未鉴权的页面，其 bootstrap POST 失败。这是预期行为，与 nonce 过期的结果一致。
