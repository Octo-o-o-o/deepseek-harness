# Agent Note: 从 sidecar 页面读取 Tauri 注入的 invoke

Status: implemented

[English](2026-08-16-desktop-plugin-restart-reads-injected-invoke.md) | 中文

## Problem

窗口开着时装插件，即使壳会回答 `true`，重启提示也不出现。页面只读 `window.__TAURI__.core.invoke`。那是可选的 withGlobalTauri 便利对象。Tauri 注入到每个 WebView 里的 command 函数——也是 `@tauri-apps/api/core` 所包装的那一个——是 `window.__TAURI_INTERNALS__.invoke`，由小得多的 `core.js` user script 送达。便利对象缺失时，每次轮询都回报「无待生效项」，抛出的 invoke 也被吞掉，所以人既看不到按钮，也看不到错误。

产品规则没有问题：入口本应只在 profile manifest 与本次 sidecar 所组合的内容不一致时出现（[功能](../feature/2026-08-16-desktop-plugin-restart-prompt.md)）。页面从未去问真正被注入的那个函数。

## Decision

`packages/client/ui-plugin-restart/src/client/shell.ts` 先从 `__TAURI__.core` 解析 invoke（文档中的 withGlobalTauri 路径），该函数缺失时再从 `__TAURI_INTERNALS__` 解析。两者都不在，仍表示页面不由壳托管。命令失败时仍回答 `false`，因此一条损坏的 IPC 路径不能把人无法操作的重启提示钉在侧栏上。

所属产品规则不变：入口仅在相对本次 sidecar 启动已过期时出现，确认后仍重启应用，浏览器里的 `dsh web` 仍然两个全局对象都没有。

## Alternatives considered

**无条件显示重启入口。** 违反所属功能的可见性规则：这个控件是为了说明组合已过期，不是提供通用重启。

**经 HTTP 问 sidecar 插件是否待生效。** 在所属功能说明里已被否决：这件事的两半都属于壳，而且重启仍需要 IPC——按钮能经 HTTP 出现、点下去却重启失败，比没有按钮更糟。

**依赖 `@tauri-apps/api`。** 该包的 `invoke` 是对 `__TAURI_INTERNALS__.invoke` 的薄包装。把它拉进一个同时进入 CLI payload 的 harness 客户端插件，换不来额外约定，却给必须在 `dsh web` 上保持惰性的包加上一套仅桌面用的运行时。

**用静态 capability 的 `http://127.0.0.1:*` 授予这两个 command。** 在所属功能说明里已被否决：等于把重启交给任何能绑定 loopback 端口的本机进程。

**invoke 抛错时钉一条损坏状态横幅。** 不可达的壳不得钉上人无法使用的控件；既有的「回答 false」规则保持不变。

## Verification

`pnpm vitest run packages/client/ui-plugin-restart`——两个文件共 19 个测试。shell 套件覆盖缺失宿主、仅便利对象、仅 internals、两者并存（便利对象优先）、抛错命令回答「无待生效项」，以及经任一路径转发的重启。

尚未覆盖：以打包应用证明 WebView 实际暴露 `__TAURI_INTERNALS__.invoke` 而省略 `__TAURI__.core`。下一个签名构建必须确认窗口开着时执行 `dsh plugin add` 后提示会出现。

## Consequences

窗口开着时改写 profile manifest 的插件安装，只要任一注入的 invoke 路径回答 `true`，就可以显示侧栏入口。浏览器标签页上的 `dsh web` 仍然两个全局对象都没有，入口在那里保持缺席。壳的时间戳、运行时 capability 与两个 command 的授予不变。
