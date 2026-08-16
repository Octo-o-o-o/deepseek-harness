# Agent Note：装了插件却未组合时提供重启入口

Status: implemented

[English](2026-08-16-desktop-plugin-restart-prompt.md) | 中文

## Problem

往 profile 里装一个插件，运行中的桌面应用毫无变化，屏幕上也没有任何东西说明这一点。`dsh plugin add` 会改写 profile manifest 的 `dsh.profile.bundles`，但组合在启动时只读了那份清单一次：`composeLive` 每次变更只重读 patch 文件，别的都不读，因此 bundle 层是一份启动快照。插件装上了，它的 UI 从不出现，而唯一的补救办法——退出再打开——没有写在任何地方。

## Decision

侧栏底部新增一个入口，仅在 profile manifest 与运行中的 sidecar 所组合的内容不一致时可见，确认后重启应用。

**热载是被否决的，不是被推迟的。** 三条互相独立的阻碍，任何一条单独成立即可否定它：

- 客户端 HMR 接收端会拒绝它尚未持有的 entry：`reload()` 在 loader tree 里查该 id，查不到就打一条警告后返回（`packages/client/hmr/src/client/index.ts`）。它为已挂载的插件更换 bundle，不接纳新插件。它的 `graph` 帧被明确标注为未使用。
- `window.__DSH_BOOT__` 是注入到所服务 index 中的，浏览器侧的 loader tree 由它构建，因此新 entry 需要一份新文档。
- 重组 host 树需要根 Include entry，而它存放在 `dsh-app-boot` 模块私有的 `WeakMap` 里。要拿到它，就得为一个桌面 Consumer 扩大共用包的公开面。

**检测属于壳。** 壳在启动 sidecar 时给 profile manifest 的修改时间打快照，按需比对。这不需要与 sidecar 就「什么算已安装」达成一致，不需要比较墙钟时间，也不需要 host 侧插件。用不相等而非先后：编辑器写入更早的时间戳，或从备份恢复，同样意味着组合已与磁盘上的内容不符。

**页面经运行时 capability 到达壳。** sidecar 页面对 Tauri 而言是远程 origin，在 capability 指名它之前调不到任何 command。端口由 OS 分配，因此壳在端口已知后再注册 capability（`dynamic-acl`），指名那个确切 origin 与恰好两个 command。若在静态 capability 里用通配端口，就等于把这两个 command 交给任何能绑定 loopback 端口的本机进程——正是 bootstrap token 所要解决的「loopback 不携带身份」问题。浏览器半侧有 `window.__TAURI__.core.invoke` 时走它，否则走 `window.__TAURI_INTERNALS__.invoke`——那是 Tauri 注入到每个 WebView 里的 command 函数，也是 `@tauri-apps/api/core` 所包装的那一个。withGlobalTauri 便利对象不是提示出现的前提（[原因](../bug-fix/2026-08-16-desktop-plugin-restart-reads-injected-invoke.md)）。

**确认弹窗是无条件的。** 重启会停掉整个本机会话进程，而浏览器没有跨会话的在途视图：`SessionListState` 带有 ids、摘要、phase 与 jobs，没有 running 标志。弹窗写明代价——进行中的回答会被中断，已保存的记录不会——而不是去猜是否存在这样的回答。

**该插件由壳自己的 patch 层激活。** `apps/desktop/desktop.patch.yml` 以 `--patch` 传入，因此该行绝不会到达 `npx @deepseek-ai/dsh web`，本 fork 相对上游的 diff 也就留在上游没有的文件里。有一行上游改动无法避免：该包必须是 `@deepseek-ai/dsh` 的依赖才能进入 sidecar payload。它在那里是惰性的——没有 patch 层就没有任何东西组合它。

## Alternatives considered

**用通配的 remote capability（`http://127.0.0.1:*`）。** 更简单，也不需要 feature flag。但 Tauri 自己的测试只覆盖 hostname 通配，没有通配端口的用例，因此其语义只能靠假设而非确知——而且该授权会覆盖 loopback 的所有端口，而不是本次启动的那一个。

**在 sidecar 里检测，经 `/api` 方法或专用 RPC channel 上报。** sidecar 若不重读并重新组合 manifest，就看不见自己已经过时，而那正是要避免的工作；它也无法重启应用。这件事的两半都属于壳。

**监听 manifest 并推送事件，而不是轮询。** 壳没有通往页面的事件通道。为一个五秒一次的询问去建一条，活动部件比这个问题本身还多。

**扩展 `composeLive` 使其重读 bundle 清单。** 它位于 `apps/cli`，每个表层都会运行，因此会改变 `dsh web` 在 manifest 被写入时的行为——而且客户端半侧仍然挂不上新 entry，插件依旧不可见。

**把入口放进托盘而不是侧栏。** 不需要新包、不需要 capability、不需要 IPC。否决的理由是：装插件的人看的是窗口，不是菜单栏。

## Verification

`pnpm vitest run packages/client/ui-plugin-restart`——两个文件共 19 个测试，该包无未覆盖的行或分支。

这些用例断言的是设计的承诺而非代码的写法：桥接层在壳之外、命令失败时、以及答案不是恰好 `true` 时都回报「无待生效项」，因此不可达的壳永远不会把横幅钉在侧栏上；入口在壳报告变更之前不可见；点击入口只打开弹窗而不重启；取消关闭弹窗且不发出任何命令；只有弹窗的确认才会到达 `restart_for_plugins`；被拒绝的重启会显示失败并保持弹窗打开；挂载后才发生的变更会被轮询接住，而其定时器随组件一同销毁；卸载后到达的答案不设置任何状态。注册与词典都随插件 fiber 释放，通过 dispose 验证。

壳侧：`cargo fmt --check`、`cargo clippy --all-targets -- -D warnings` 与 `cargo test` 通过，最后一项覆盖 manifest 路径形状、缺失 manifest 永不判为过时、以及被改写的 manifest 判为已变更（时间戳显式设置，因为一个文件系统时间戳刻度内的两次写入会比较相等）。

壳侧有两个测试 `lock::tests::second_lock_is_busy` 与 `health::tests::host_describe_accepts_a_chunked_reply` 在负载下偶发失败——每次失败的是其中不同的一个，单独跑与重跑都通过。它们早于本次改动，与之无关。

## Consequences

装了插件的人现在会知道需要重启，并且一次点击即可完成，而不必退出再打开。他们得不到的是「不重启就用上插件」；要做到那一点，上述三条阻碍都得被解除，而前两条是浏览器半侧启动方式的固有性质。

桌面壳现在有了一个可从 sidecar 页面到达的命令面，这是此前没有的。它只有两个 command，作用域限定在每次启动都会变的那一个 origin 上，且在端口已知后才授予。`notify_attention` 仍不可达——它从未被授予 capability——因此日后要接通它是一次刻意的动作，而不是本次改动的副产物。

轮询在窗口存续期间每五秒问一次。那是一次到同机进程的 IPC 往返，并且随组件停止。
