# Agent Note: 侧边栏品牌行标示桌面应用

Status: implemented

[English](2026-08-14-desktop-brand-row.md) | 中文

## Problem

桌面外壳提供的正是浏览器表面所提供的同一套 Web 前端，因此它的侧边栏打开时显示的是 Web 字标——那是一个站点的标识，而不是用户双击启动的那个应用的标识。浏览器平面中没有任何东西知道当前跑的是两者中的哪一个，而前端也没有按表面区分的插件配置：客户端行经由 `window.__DSH_BOOT__` 到达浏览器，该 wire 只携带 `id`/`url`/`rev`/`inject`，不带 `config`，所以这里无法用 cordis.yml 的取值来选择形态。

## Decision

`ctx.connection` 发布 `isDesktopShell`，这是一个从外壳 sidecar 注入到其所服务 index 中的 bootstrap 标记派生出来的页面事实。它与 `isLoopback` 并列：两者都由页面派生、在页面生命周期内固定，并且在首帧之前即可读取，因此任何表面都不必等待连接握手，也不会在握手后再换形态。

`ui-sidebar` 注入 `connection`，通过其 slot inject 面传递 `desktopShell`，并渲染两种品牌形态之一。在外壳中，该行是 26px 的 `AppMark` 加上分作两行的产品名——主墨色的 `DeepSeek Harness` 在上，次级墨色的 `Desktop` 在下——因为单行名称在标识旁放不进 300px 的栏宽而不被截断。其余一切不变：该行仍是 New Session 快捷方式，收起后的轨道在两种表面上完全一致。

`AppMark`（ui-primitives）把打包图标绘制为矢量：白色鲸鱼镂空在图标的深色底板上，另加一道细边框，使底板边缘在深色侧栏上依然可辨。它是唯一使用字面颜色而非 `--dsw-*` 令牌的原子组件——它标识的是已安装的应用，而该图标在 Dock 中于两种主题下都是同一个样子。鲸鱼路径已移入 `src/whale-path.ts`，与 `FishLogo` 共用。

## Alternatives considered

**在 ui-sidebar 里直接读取该标记。** 这样可以省掉新增依赖，但会让标记名在代码树中出现第三份副本（web-app 的宿主半边与 connection 的客户端半边已各持一份），并让一个 UI 包知晓 bootstrap 细节。

**给 `host.describe` 增加外壳字段。** 该描述要到握手之后才到达，品牌行会先渲染 Web 字标再切换；而且这个事实属于页面而非宿主——同一个 sidecar 也服务浏览器标签页。

**给客户端 boot wire 增加 `config` 字段。** 这是符合组合形态的答案，但该 wire、它的解析器以及每一个消费方的存在都是为了承载代码身份；一处表面差异不足以让它们变宽。

**把图标 PNG 以 data URI 内嵌。** 该标识必须在 26px 下于两种主题中都清晰渲染，并与随包发布的图标保持同步；矢量加共用鲸鱼路径可以保持几何数据的单一来源。

## Consequences

桌面窗口打开时带着自己的图标与完整产品名；浏览器标签页不受影响，因为其 `isDesktopShell` 为 false。覆盖范围：标记读取及其空字符串情形、两种页面状态下的 handle 字段、插件的 inject 列表与注入的页面事实、外壳组件中的两种品牌形态，以及桌面品牌行的 slot runtime 快照。未来若还有必须在打包应用内表现不同的表面，读取同一个标志即可，不必再新增通道。
