# Agent Note: Desktop alignment onto dsh 0.1.0-rc.8

Status: implemented

[English](2026-08-20-desktop-rc8-alignment.md) | 中文

## Problem

本 fork 的桌面壳监管 `dsh web` 并加载 sidecar 的 Web GUI。上游 `0.1.0-rc.8` 改了这套组合：本机 `dsh web` 除非传入 `--no-open` 否则会打开默认浏览器；侧栏品牌行变成通用 slot，fallback 为「DSH Local Build」；schema 15 的 SQLite 会话文件在 schema 17 下被直接拒绝且无迁移。分叉停在 rc.7 会让打包 GUI 吃不到这些产品变化。合入却不改编桌面组合，则会在 WebView 旁边再打开一个未配对的浏览器标签、丢掉打包应用的 AppMark 行，并把 `apps/desktop` 当成可发布成员。

## Decision

分叉 merge-forward 到 `dsh-v0.1.0-rc.8`，在未设置 `DSH_DESKTOP_TOKEN` / `DSH_DESKTOP_BOOTSTRAP_NONCE` 时非桌面表层保持不变。

桌面组合绝不把 loopback URL 交给系统浏览器。`handoffBrowser` 为 `config.openBrowser && !ssh && desktop === undefined`。sidecar argv 同时钉死 `--no-open`（含打包自检），因此即便漏掉一处代码路径也不会去 spawn `open`。

侧栏壳采用 rc.8 的 slot 模型。打包应用的图标与两行名称由 `@deepseek-ai/dsh-client-ui-plugin-restart` 占据 `sidebar.brand.mark`、`sidebar.brand.name` 和 `conversation.hero.brand.mark`，由 `apps/desktop/desktop.patch.yml` 挂载。浏览器里的 `dsh web` 不加载该插件，因此仍是官方 occupant 或 Local Build fallback。

`registerGuard`、desktop bootstrap 与分享网关仍是 `dsh-web-app` 里仅桌面会走到的路径。ArrowUp 提问召回留在 `InputBar`，排在 rc.8 引用芯片按键处理之后。发布成员目录仍是 `apps/(cli|web)` 加上非 experimental 包，因此 `apps/desktop` 保持 private。

默认会话持久化仍是 JSONL。rc.7 的 SQLite 文件与 rc.8 不兼容；本组合不迁移它。

## Alternatives considered

**在 `SidebarRoot` 里保留 `if (desktopShell)`。** 作为仅桌面执行路径是允许的，但上游以后每次改品牌 slot 都会在同一行冲突。把 occupant 放在已有的仅桌面插件上，才能复用 rc.8 的通道。

**只靠 `--no-open`，或只靠 `handoffBrowser`。** 任意一种都会漏掉另一处调用方（打包自检 vs 手搭的树）。两道都要。

**新增 `ui-brand-desktop` 包。** 为三个 slot 注册再开一个客户端包，等于重复桌面 patch 层已经在做的事：把桌面 UI 留在共用 web 组合包之外。

**把发布成员正则放宽成 `apps/[^/]+`，再靠 `private: true`。** rc.8 把 private 的发布成员判为错误。该目录必须留在发布集合之外。

## Consequences

打包启动不会再在 WebView 旁边打开 Safari/Edge。品牌行是 occupant，因此若将来桌面构建用上 official profile，会在同一组 single slot 上碰撞，除非该 profile 保持关闭。把 sqlite 持久化插件指到 `~/.dsh` 的用户无法在此构建中打开那些文件；JSONL 用户不受影响。下次桌面包之前必须相对 rc.8 重生 payload-manifest 的 `cli`。

## Testing

`packages/bundle/web-app/tests/web-app.spec.ts` 断言 desktop bootstrap 仍会注册，且设置成对桌面环境变量时不会调用 `internals.openBrowser`。`packages/client/ui-plugin-restart/tests/browser-plugin.client.spec.tsx` 断言三个品牌 hole 会填入并随 fiber 撤走。`cargo test desktop_args_pin_loopback` 钉死 sidecar argv 上的 `--no-open`。`scripts/check-workspace-constraints.spec.ts` 拒绝把 `apps/desktop` 当作发布成员。
