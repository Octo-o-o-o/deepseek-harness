# dshd

[English](README.md) | 中文

Tauri 2 壳：在 `127.0.0.1` 上启动本地 `dsh web` sidecar，等待就绪行，校验 `__DSH_BOOT__` 与 `host.describe`，随后在 WebView 中加载既有的 Web GUI。

```
┌─────────────────────────────────────────────┐
│  Tauri 2 shell (tray, single-instance)      │
│    spawn node → parse ready line            │
│    GET /  +  POST /api/host.describe        │
│    navigate WebView to http://127.0.0.1:N   │
└──────────────────┬──────────────────────────┘
                   │ loopback only
                   ▼
┌─────────────────────────────────────────────┐
│  Node sidecar (bundled runtime + deploy)    │
│    dsh web --port 0 --host 127.0.0.1        │
│    env DSH_DESKTOP_TOKEN + BOOTSTRAP_NONCE  │
└─────────────────────────────────────────────┘
```

## 开发

在本目录下，且已构建 CLI（仓库根执行 `pnpm run build`）：

```sh
cd src-tauri
cargo test
cargo run
```

`cargo run` 使用 PATH 上的 `node` 与检出中的 `apps/cli/lib/bin.js`。可用 `DSH_NODE_PATH` / `DSH_WEB_BIN` 覆盖。`DSH_HOME` 与 `DSH_WORKSPACE` 覆盖数据目录与 sidecar 的 cwd。

门禁（cwd = `src-tauri`）：

```sh
cargo fmt --check
cargo clippy -- -D warnings
cargo test
```

## 打包

```sh
# repo root: production deploy + pinned Node v24.19.0 + PATH-stripped boot
pnpm --filter @deepseek-ai/dshd run pack

# this package: unsigned .app, then re-copy the sidecar (Tauri drops symlinks)
pnpm --filter @deepseek-ai/dshd run build
```

`scripts/pack-sidecar.mjs` 的步骤：`deploy`、`runtime`、`check`、`embed`（在 `tauri build` 之后）。自检要求 15 秒内出现就绪行、`GET /` 返回 200 且含 `__DSH_BOOT__`、SIGTERM 退出码 0，且 `PATH=/usr/bin:/bin:/usr/sbin:/sbin`。

这些探测使用 `fetch` 与 `curl`，二者会解码 `Transfer-Encoding: chunked`——sidecar 始终发送的框架——而壳自己的健康检查客户端在 `http.rs` 中解码它。把该客户端约束到真实框架的是 `cargo test`，不是打包自检。

打包后的 macOS `.app` 通常约 320MB（Node + 生产 deploy）。Windows 下解包 Node zip 已在打包脚本中实现；在本机只由 CI 运行。

## 数据目录与日志

| | macOS | Windows |
|---|---|---|
| `DSH_HOME` | `~/.dsh` | `~/.dsh` |
| sidecar cwd | `desktop-state.json` → `workspace`，否则 `~/Documents` | 同上，否则用户主目录 |
| sidecar 日志 | `$DSH_HOME/logs/sidecar.log`（50MB 轮转） | 同上 |
| panic 日志 | `$DSH_HOME/logs/crash.log` | 同上 |
| 锁 | `$DSH_HOME/desktop.lock`（`flock`） | 排他 `share_mode(0)` |

数据目录就是 npm 版 CLI 使用的那一个，因此会话、设置与工作区与 `npx @deepseek-ai/dsh web` 实时共享；两者可同时运行，各自使用系统分配的端口。`desktop.lock` 仅由本壳持有，因此它排斥第二个 `dshd`，而不排斥 CLI 服务。

当 `migration-state.json` 不存在、且 `~/.dsh` 尚未持有这些目录时，首启会从统一前的桌面目录——`~/Library/Application Support/DeepSeekHarness`、`%APPDATA%\DeepSeekHarness` 或 `DSH_LEGACY_HOME`——复制 `sessions`、`settings`、`attachments`、`storages`、`profiles`，因此已有的 CLI 数据绝不会被覆盖。凭据不复制。失败会恢复 `migration-backup-<ts>`。`DSH_DESKTOP_MIGRATE_FAIL=1` 为测试注入该失败。拿不到锁的第二个进程显示"另一个 DeepSeek Harness 进程正在使用数据目录"，并且不拉起 sidecar。

`sidecar.pid` 记录 sidecar 的进程号与入口脚本。下次启动只在两者仍然匹配时才回收该进程，因此被 CLI 版 `dsh web` 复用的进程号不会被误杀。

## Token

壳始终生成每次启动独立的十六进制 token 与 bootstrap nonce，并作为 `DSH_DESKTOP_TOKEN` / `DSH_DESKTOP_BOOTSTRAP_NONCE` 注入。Web index 只拿到 nonce；`POST /__dshd_bootstrap` 为 `/api` 设置 HttpOnly 的 `dsh-token` cookie。壳的自检使用 `X-DSH-Token`，并在 WebView 客户端 POST `/__dshd_ready` 之后等待 `/__dshd_status`。token 不会进入 argv、URL 或日志。不带这些环境变量的 `dsh web` 行为不变。

## 已知限制

- Windows 的进程树终止（Job Object）与 `share_mode(0)` 锁已编译但**未在本机验证**。本环境的 `rustup target add x86_64-pc-windows-msvc` 失败（rustup 缓存）。Windows 运行由 CI 负责。
- Windows 沙箱仍不完整，与 CLI 相同。
- 未接入 WebView2 存在性检测 / 安装器提示。
- `.app` 未签名；公证不在范围内。
- 从沙箱内 `open` 该 `.app` 可能失败（`LSOpen` -54）；直接启动 `Contents/MacOS/dshd` 仍能拉起 sidecar。
