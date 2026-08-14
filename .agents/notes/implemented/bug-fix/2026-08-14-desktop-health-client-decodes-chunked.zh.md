# Agent Note: 桌面健康检查客户端解码 chunked 响应

状态：已实施

[English](2026-08-14-desktop-health-client-decodes-chunked.md) | 中文

## 问题

`apps/desktop/src-tauri/src/http.rs` 把首部终止符之后的全部内容当作响应体返回。sidecar 的 Node 服务不设 `Content-Length`，index 与 `/api` 的每个响应都用 `Transfer-Encoding: chunked`，因此到达健康检查的响应体仍带着分块框架（`e7\r\n{…}\r\n0\r\n\r\n`）。

`check_loader_ready` 只做 `__DSH_BOOT__` 子串匹配，该子串在框架中依然存在，所以第二道门通过。`host_describe_ok` 与 `status_ready` 要解析 JSON，而带框架的文本不是 JSON，于是 `check_host_described` 每次启动都以 `host.describe result is not ok` 失败，`wait_desktop_client_ready` 也永远看不到 `ready: true`。桌面壳根本无法启动。

现有门禁都看不见它。`health.rs` 与 `http.rs` 的单元测试提供的是 `Content-Length` 响应体；两处面向真实服务的检查——`pack-sidecar.mjs` 的 `selfCheck` 与 `smoke-app.sh`——分别用 `fetch` 和 `curl` 探测，两者都会透明解码 chunked。唯一不解码 chunked 的客户端，恰恰是唯一从未指向真实服务的客户端。

## 决定

`parse_http_response` 在原始字节上切分首部与响应体，从首部读取 `Transfer-Encoding`，声明为 `chunked` 时经 `decode_chunked` 拼接各分块载荷。identity 响应体保持原样。按字节切分使分块长度与线路字节对齐，多字节 UTF-8 下 `String` 层切分无法保证这一点。框架损坏或截断返回 `HttpError::InvalidChunkedBody`，而不是交出一个会被静默判成 not-ok 的响应体。

分块长度之后的 chunk extension 被跳过，trailer 段不读取：响应在零长度分块处即已完整，且没有任何健康检查消费 trailer。

回归测试放在 `health.rs`：`serve_once_chunked` 按 sidecar 的方式给响应加框架，两道 JSON 门禁从此对着真实框架校验，而不是只有这个客户端才会接受的夹具。

## 考虑过的替代方案

**让 sidecar 在健康路由上返回 `Content-Length`。** 那是为一个内部客户端改动产品的 HTTP 表面来修这两道门，而任何新增路由都会重新引入该缺陷。

**用子串匹配代替解析 JSON。** `body.contains("\"ok\":true")` 能穿过框架，但也会接受来自无关字段的嵌套 `ok`，并丢掉"响应格式良好"这一保证。

**引入 Rust HTTP 客户端 crate。** 该壳有意为三个 loopback 探测不带 HTTP 依赖；[依赖优于手写策略](../process/2026-07-26-dependencies-over-hand-rolling.md)用依赖换取自有代码，而四十行带测试的分块解码达不到这个门槛。

## 影响

四道门的启动流程走通：打包后的 `dshd.app` 能拉起 sidecar、通过 `host.describe`、导航 WebView 并观察到 `/__dshd_ready`。此后为该客户端新增的任何健康路由，无论服务端选哪种框架都能正确解码。
