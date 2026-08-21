# Agent Note: Desktop alignment onto dsh 0.1.1-rc.1

Status: implemented

[English](2026-08-21-desktop-rc11-alignment.md) | 中文

## Problem

本 fork 的桌面壳监管 `dsh web` 并加载 sidecar 的 Web GUI。上游 `0.1.1-rc.1` 带上了视觉 catalog 模型，把临时的 index.html tap 换成结构化注入表加 `renderIndex`，并把缺失的 SPA 路径从 200 回写 `index.html` 改成 404。合入时若不保住仅桌面席位，会丢掉 `registerGuard`、ArrowUp 提问召回，以及 `apps/(cli|web)` 发布成员正则。

rc.8 的 overlay、品牌 patch 和 `--no-open` argv 仍然需要；那条决策在 [rc.8 对齐说明](2026-08-20-desktop-rc8-alignment.zh.md)。

## Decision

分叉 merge-forward 到 `origin/master` 的 `0.1.1-rc.1`，在未设置 `DSH_DESKTOP_TOKEN` / `DSH_DESKTOP_BOOTSTRAP_NONCE` 时非桌面表层保持不变。

`registerGuard` 仍然赶在每次 HTTP 匹配和 upgrade 之前运行。`tapIndex` 仍是 `renderIndex` 写完结构化 `IndexInjection` 行之后的逃生口；桌面 bootstrap 脚本继续经 `tapIndex` 注册。ArrowUp 提问召回留在 `InputBar`，与新增的编辑区间记账并存。发布成员目录仍是 `apps/(cli|web)` 加上非 experimental 包。

打包组合仍关掉 `ui-brand-official`，并把 sidecar argv 钉死为 `dsh web --port 0 --host 127.0.0.1 --no-open`。桌面健康检查仍要求 `GET /` 200 且含 `__DSH_BOOT__`；boot 赋值现在是 `globalThis["__DSH_BOOT__"] = …`，该子串仍然匹配。SQLite `SCHEMA_VERSION` 仍是 17；`SESSION_FORMAT_VERSION` 仍是 0。

视觉支持是 catalog 行 `deepseek-v4-flash-vision-exp`，`inputModalities: ['text', 'image']`。桌面 WebView 与 Tauri capability 不新增图片通路：`ui-attachment` 已经负责接入。

## Alternatives considered

**整份采用上游的 `webserver` / `InputBar` / 发布成员正则。** 那会删掉准入席位、ArrowUp 召回，或把 `apps/desktop` 当成可发布成员。这三项都是 rc.8 合入留下的仅桌面义务。

**把桌面 bootstrap 从 `tapIndex` 改成 `IndexInjection` 行。** nonce 门控脚本是行无法表达的标记；`tapIndex` 就是结构化行之后文档写明的逃生口。

**在同一改动里把插件 peer 钉死为精确的 `0.1.1-rc.1`。** 桌面用户和 `dsh@next` 用户会对不上。部署插件改为同时订阅 `credentials/updated` 与 `credentials/reference-updated`。

## Consequences

打包启动仍然不会打开系统浏览器，仍然经桌面 patch 占据品牌 slot，仍然拒绝非回环 sidecar 绑定。dist 根下缺失的静态文件返回空 404；壳只导航 `GET /`，因此这一变化不改变首屏。仅桌面包 `@deepseek-ai/dsh-client-ui-plugin-restart` 继续跟 dsh 发布族同一版本号，否则 `scripts/release/pack.ts` 打不成 sidecar。打包后 `payload-manifest.json` 的 `cli` 为 `0.1.1-rc.1`。Sidecar pack、桌面 CI 和签名 macOS 发布继续跑 `pnpm run build:official`。

## Testing

`packages/bundle/web-app/tests/web-app.spec.ts` 断言 desktop bootstrap 仍经 `tapIndex`/`renderIndex` 注册，且设置成对桌面环境变量时不会调用 `internals.openBrowser`。`packages/host/webserver/tests/webserver.spec.ts` 同时覆盖准入 guard 与现收的 `webserver/index-inject` 收集。`packages/client/ui-conversation/tests/input-bar.client.spec.tsx` 同时覆盖 ArrowUp 召回与编辑区间。`apps/desktop/tests/desktop-patch.spec.ts` 断言 `ui-brand-official` 被关掉。`cargo test overlay` 拒绝漏掉 `--no-open` 的 argv。`scripts/check-workspace-constraints.spec.ts` 拒绝把 `apps/desktop` 当作发布成员。Sidecar pack 把 `payload-manifest.json` 的 `cli` 记为 `0.1.1-rc.1`，自检要求就绪行、`GET /` 200 且含 `__DSH_BOOT__`、以及干净的 SIGTERM。
