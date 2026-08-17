# dshd 发版看板（RC7 对齐 · 0.1.12）

> **SoT。** 本文件是 2026-08-17 这一轮桌面发版的对照表与推进记录。它取代聊天里的缺口枚举，并 supersede 把下列来源当「当前权威」来用：`proposals/desktop-mac-gap-review.md`（8/14 第一轮）、`proposals/desktop-mac-crossreview.md`、[桌面壳质量缺口](e8a088df-594b-4594-b0ad-9219dd3e6a97) 扫描、`proposals/2026-08-17-desktop-rc7-compat.md`（实施计划，事实以本板「已完成」为准）。那些文件仍是历史证据，不是本轮是否上船的判决。

**本轮产物：** 公证 macOS DMG `dshd-0.1.12-arm64.dmg`，推到 GitHub Release `dshd-v0.1.12`，并更新 `dshd-dl.octoooo.com` 的安装包与 `/updates/latest.json`。

**Fork 红线：** 未设置 `DSH_DESKTOP_TOKEN` / `DSH_DESKTOP_BOOTSTRAP_NONCE` 的 `dsh` / `dsh web` 观察行为不得变。

---

## 本轮范围

上船：RC7 载荷、WKWebView Safari 身份、updater 失败不再先拆 sidecar、splash 可重启、手势返回不进死 splash、分享网关（工作区已成型）且 Tailscale 在 HTTPS 就绪前不发码、壳版本 0.1.12。

明确留到下一轮（不挡公证 DMG）：审批通知接到渲染端、崩溃后不整进程重启而只拉 sidecar、Unix `kill -9` 孤儿、就绪 15s 偏紧、运行期日志轮转、`pack-sidecar.mjs` 单测、Intel Mac、Windows 签名发版、updater 真机「旧装 → 新装」一次（本轮只保证产物与 `latest.json` 写得出）。

---

## RC7 对照

| RC7 能力 | 桌面怎么对应 | 状态 |
|---|---|---|
| 插件自注册设置卡、Job Panel、MCP/ACP 图片、PTC 更名、`low` effort、可折叠提问卡、极简 Bash | 共享 Web GUI，dshd 只是宿主 | 继承，无需桌面代码 |
| Safari textarea 软换行恢复 | 系统 WKWebView 默认 UA 过不了 `isSafariBrowser` | **已修**：macOS 主窗口钉 Safari.app UA |
| `node-pty` 1.2.0-beta.15 | `payload-manifest.json` 曾停在 1.1.0 | **已修**：darwin 重录；win32 共享版本对齐 |
| 非桌面 `dsh web` | 无桌面 env 时 bootstrap / guard / `/p` 仍是 SPA | **已测**：`GET /` 无 `__DSH_DESKTOP_BOOTSTRAP__`；`/api` 非 401 |

---

## 已完成（本会话已有证据）

| ID | 项 | 证据 |
|---|---|---|
| D1 | merge-forward 上游 `0.1.0-rc.7` | `e862dbfdcc` |
| D2 | macOS WebView Safari.app UA | `webview_identity.rs`；本机 WKWebView custom UA → `isSafariBrowser` true |
| D3 | payload-manifest rc.7 + `node-pty@1.2.0-beta.15` | `pack-sidecar.mjs manifest` + `check` native ok |
| D4 | 桌面组合：token / nonce / patch / plugin-restart | 带桌面 env 的 `dsh web --patch`：401 → bootstrap 204 → `/api` 放行 |

---

## 本轮必须推进

| ID | 项 | 严重度 | 源码事实 | 目标 | 状态 |
|---|---|---|---|---|---|
| S1 | Updater 先 `request_stop` 再下载，失败后 sidecar 已死 | 高 | `update.rs` `run_install` | 先 `download`（含验签）再停 sidecar 再 `install`；`install` 失败则回启动页 | **已修** |
| S2 | 侧车崩了 / 启动失败只有「打开日志」，不能重启 | 中高 | `frontend/index.html`；`wait_for_unexpected_exit` 后 `show_error` | splash 错误态提供 Restart，走已有 `restart_for_plugins`（整进程重启） | **已修** |
| S3 | 手势返回 / 历史后退回到永远 Starting 的 splash | 中 | `is_internal_url` 放行 `tauri://` | WebView 一导航到 sidecar 就拒绝回到 start page，改回 loopback origin（无 nonce fragment） | **已修** |
| S4 | Tailscale HTTPS 未进 Serve 表仍 `setTailscaleAudience` | 中 | `share.rs` `enable_tailscale` 把 `wait_https_listed` 失败打成 eprintln | 失败则停 child、不写 audience、把错误交回 UI | **已修** |
| S5 | 壳版本与文档 / 上次 Release 标签错位 | 中 | 壳已是 0.1.11；GitHub 标签停在 `dshd-v0.1.7`；README stapler 仍写 `0.1.0` | 发 **0.1.12**；README / 官网 `release.js` 跟到该文件名 | 壳与 README 已改；`release.js` 等 DMG SHA |

---

## 下一轮（不进 0.1.12）

| ID | 项 | 为什么可以晚 |
|---|---|---|
| N1 | `notify_attention` 接到 ApprovalPanel + sidecar capability | 半截功能，修法是完整产品面，不是发版阻塞 |
| N2 | 崩溃后只重拉 sidecar、不 `app.restart()` | S2 用整进程重启兜住 |
| N3 | Unix 父进程 `kill -9` 留孤儿 Node | 下次启动 `reap_stale_sidecar`；Windows 已有 Job Object |
| N4 | 就绪等待 15s vs 冷启动 | 本机 pack check 与 CLI 探针均在 15s 内就绪；Windows 首启另测 |
| N5 | 运行期 `sidecar.log` 轮转 | 启动时已按 50MB 切一次 |
| N6 | `pack-sidecar.mjs` 单测 | 发版脚本仍人手跑 `manifest`/`check` |
| N7 | Intel / Windows 签名安装包 | 本轮只出 arm64 公证 DMG |

---

## 发版与验收清单

- [x] S1–S5 落地，聚焦测试绿（`cargo test` 99；clippy `-D warnings`；share-gateway / safari / desktop-bootstrap / ui-plugin-restart 72）
- [x] 代码复审（缺陷优先）：安装失败回 splash + `sidecar_url` 提前写入，已闭合；无剩余发版阻断
- [ ] 提交并 `git push fork master`（用户要求上 Release）
- [ ] `pnpm run release:desktop-mac` 公证 DMG
- [ ] `stapler validate` + Gatekeeper `spctl`
- [ ] GitHub Release `dshd-v0.1.12` 附 DMG、SHA256、`.app.tar.gz` + `.sig`
- [ ] 上传 R2 内容寻址 key，写 `/updates/latest.json`，更新 `site/release.js` 并部署 Pages
- [ ] 本机打开公证 DMG：能启动、composer 在 WKWebView 下 `isSafariBrowser` 为 true

---

## 推进记录

- 2026-08-17：看板落盘。D1–D4 已在同日完成。
- 2026-08-17：S1–S5 落地。复审发现安装失败后 Restart 不可达、以及 client-ready 等待期间 swipe-back 窗口，均已修。`dsh-notary` keychain profile 仍能拉到 Accepted 历史。
