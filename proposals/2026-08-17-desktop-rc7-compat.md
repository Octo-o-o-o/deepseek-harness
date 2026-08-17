# 桌面端与上游 0.1.0-rc.7 兼容实施计划

> 本会话直接按此计划实施。不单独开 worktree。不提交，除非用户明确要求。

**Goal:** 让 `apps/desktop`（dshd）在本机与当前仓库里的上游 `0.1.0-rc.7` 行为对齐：RC7 的 Web GUI 能力在桌面 WebView 里可运行，打包载荷与 rc.7 闭包一致，且 `dsh` / `dsh web` 在未设置桌面环境变量时观察不到任何变化。

**Architecture:** RC7 的产品能力（插件设置卡、Job Panel、MCP 图片、PTC 更名、`low` effort、可折叠提问卡、极简 Bash 延迟修复）已经在共享 Web GUI 里，桌面只是宿主，不需要再实现一遍。真正会让「看起来像没吃到 RC7」的，是两处桌面特有缺口：(1) macOS WKWebView 的默认 UA 过不了 `isSafariBrowser`，RC7 的 Safari textarea 恢复在 dshd 里不跑；(2) `payload-manifest.json` 仍记录 rc.5 时期的外部包（含 `node-pty@1.1.0`），而 lockfile 已是 `node-pty@1.2.0-beta.15`，打包门禁会失败。修复只走桌面壳：给主窗口设 Safari.app 形式的 User-Agent，不改共享检测函数。

**Tech Stack:** Tauri 2.11 / wry WKWebView、`ui-conversation` 的 `isSafariBrowser`、`apps/desktop/scripts/pack-sidecar.mjs`、vitest、cargo test。

## Global Constraints

- Fork 红线：未设置 `DSH_DESKTOP_TOKEN` / `DSH_DESKTOP_BOOTSTRAP_NONCE` 的 `dsh`、`dsh web` 观察行为不得变。
- 共享包只允许：(a) 仅桌面组合走到的路径；(b) 缺席时等于没改的可选扩展点。禁止放宽或收紧 `isSafariBrowser` 来「顺便」覆盖所有 WKWebView。
- 未提交的分享网关 WIP 保持独立，不折进这次兼容修复的语义；打包清单只记录外部包版本，不把 WIP 当成发布内容。
- 不提交 git（用户未要求）。不 push。
- 非阻塞、不算 RC7 兼容失败：审批通知未接线、updater 真机升级、Intel Mac、Windows 签名、手势返回 splash。

---

### 兼容定义（验收口径）

「完美兼容」在本计划里是可证伪的，不是感觉：

1. **载荷**：`payload-manifest.json` 的 `cli` 为 `0.1.0-rc.7`；本机 `darwin-arm64` 节与一次真实 `pack-sidecar.mjs manifest` 的闭包一致，其中 `node-pty` 为 `1.2.0-beta.15`。
2. **原生**：打包树里的 `node-pty` / `sharp` / `koffi` / `node:sqlite` 能用捆绑 Node 加载（`pack-sidecar.mjs` 的 native probe）。
3. **Safari 恢复**：dshd 主窗口的 `navigator.vendor === 'Apple Computer, Inc.'` 且 UA 匹配 `\bVersion\/[\d.]+.*\bSafari\/[\d.]+`，从而 `InputBar` 会调用 `repairSafariTextareaLayout`。共享函数本身与 CLI/浏览器路径字节级不变。
4. **组合**：sidecar 仍注入桌面 token/nonce，admission guard 与 `desktop.patch.yml` 的 `ui-plugin-restart` 仍在。
5. **非桌面**：`pnpm dsh --profile web --port <p>` 不带桌面环境变量时，`GET /` 仍是普通 Web GUI，没有桌面 bootstrap 强制路径。
6. **继承 UI**：RC7 的设置卡 / Job Panel / PTC / `low` 随 Web GUI 进入 WebView，桌面不另写一套。

---

### Task 1: macOS WebView 呈现 Safari.app 身份

**Files:**
- Create: `apps/desktop/src-tauri/src/webview_identity.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`（`mod webview_identity` + `build_main_window`）
- Modify: `packages/client/ui-conversation/tests/safari.client.spec.ts`（加夹具，不改 `safari.ts`）

**Interfaces:**
- Produces: `webview_identity::MACOS_SAFARI_WEBVIEW_USER_AGENT: &str`
- Consumes: Tauri `WebviewWindowBuilder::user_agent`; `isSafariBrowser` 的既有谓词

- [x] **Step 1: 固定 UA 常量与 rust 测试**

常量必须与下面 TypeScript 夹具字节相同：

```
Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.5 Safari/605.1.15
```

本机未设 custom UA 时 WKWebView 实测为（无 `Version/` / `Safari/`，检测为 false）：

```
Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)
vendor: Apple Computer, Inc.
```

- [x] **Step 2: 仅在 macOS 给主窗口 `.user_agent(...)`**

Windows WebView2 是 Chromium，不得套这条 Safari UA。

- [x] **Step 3: 共享检测测试增加两条夹具**

`unadorned macOS WKWebView` → false；`macOS WKWebView presenting Safari.app tokens` → true。`Apple web view` 原夹具保持 false。

- [x] **Step 4: 跑测试**

```
cd apps/desktop/src-tauri && cargo test webview_identity
pnpm exec vitest run packages/client/ui-conversation/tests/safari.client.spec.ts
```

---

### Task 2: 文档（Agent Note + README）

**Files:**
- Create: `.agents/notes/implemented/bug-fix/2026-08-17-desktop-wkwebview-safari-identity.md` 及 `.zh.md` / `.i18n.yaml`
- Modify: `.agents/notes/implemented/bug-fix/2026-08-13-safari-textarea-soft-wrap-reflow.md` 及其中文对（Verification 加一句交叉链接）
- Modify: `apps/desktop/README.md` / `README.zh.md`

```
pnpm run verify-translation-pairing --write \
  .agents/notes/implemented/bug-fix/2026-08-17-desktop-wkwebview-safari-identity.md \
  .agents/notes/implemented/bug-fix/2026-08-13-safari-textarea-soft-wrap-reflow.md \
  apps/desktop/README.md
pnpm run verify-agent-note-format -- \
  .agents/notes/implemented/bug-fix/2026-08-17-desktop-wkwebview-safari-identity.md
```

---

### Task 3: 重生 payload-manifest

在 `apps/desktop` 跑：

```
node scripts/pack-sidecar.mjs manifest
```

该步骤会 `deployApp()`（打 tarball + `npm install --ignore-scripts`），然后把本机节写入 `platforms['darwin-arm64']`。`win32-x64` 节不得被整文件覆盖。

验收：`node-pty` 在 darwin 节为 `1.2.0-beta.15`，`cli` 仍为 `0.1.0-rc.7`。

若捆绑 Node 已在 `sidecar/dist/bin`，接着：

```
node scripts/pack-sidecar.mjs runtime
node scripts/pack-sidecar.mjs check
```

`check` 含 native probe（pty spawn）和 15s ready-line 自检。

---

### Task 4: 非桌面姿态 + 桌面组合回归

```
pnpm dsh --profile web --port <p>
# 不设 DSH_DESKTOP_TOKEN / DSH_DESKTOP_BOOTSTRAP_NONCE
curl -sS -D- http://127.0.0.1:<p>/ | head
```

期望：200，页面含 Web GUI，无桌面 nonce 强制 bootstrap。

桌面侧：`cargo test`（`apps/desktop/src-tauri`）；已有 `desktop-bootstrap` / `ui-plugin-restart` 测试保持绿。

---

### Task 5: 本机 WKWebView 探针（Cursor 内置浏览器不算数）

用 Swift `WKWebView.customUserAgent = MACOS_SAFARI_WEBVIEW_USER_AGENT`，读取 `navigator.userAgent` / `navigator.vendor`，交给 `isSafariBrowser`。期望 `true`。

未自动化（明确缺口）：在运行中的 dshd 窗口里用 Backspace 跨软换行复现 Safari 26.5 几何缺陷——需要抢焦点的 GUI 自动化。组件测试 + UA 门控为代用证据。

---

### 明确不做

- 把 `isSafariBrowser` 放宽到所有 Apple WebView
- 在 `desktop-bootstrap.ts` 里 Object.defineProperty 伪造 UA
- 接线 `attention::notify_attention`（RC7 之前的桌面债）
- 真机 updater 全路径
- 把分享网关 WIP 提交或折进兼容语义
- Windows 上重跑 `manifest`（本机是 darwin-arm64；win32 节保持原样，下次 Windows 打包再录）
