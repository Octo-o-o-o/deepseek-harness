# @deepseek-ai/dsh-client-ui-plugin-restart

[English](README.md) | 中文

仅桌面可见的侧栏动作：当 profile 的插件清单已经不同于运行中组合所读到的内容时，出现一个重启入口。`dsh plugin add` 会改写 profile manifest 的 `dsh.profile.bundles`，但组合在启动时只取一次该清单——`composeLive` 只重读 patch 文件——因此新装的插件在应用重新启动之前始终不生效。node 半侧是一个空 `apply`，只为让插件出现在 Loader 中；浏览器半侧经 `exports["./client"]` 交付。

这件事的两半都属于桌面壳而不属于本进程：壳在启动这个 sidecar 时给 profile manifest 打了时间戳，也只有它能替换进程。浏览器半侧经 Tauri IPC 询问（`plugins_pending_restart`）：有 `window.__TAURI__.core.invoke` 时走它，否则走 `window.__TAURI_INTERNALS__.invoke`——那是 Tauri 注入到每个 WebView 里的 command 函数。答案为真时把入口渲染到 `sidebar.footer.action`，确认后调用 `restart_for_plugins`。它不做任何判断，也不读取任何 harness 状态。在打包应用之外两个全局对象都不存在，所有调用都回报「无待生效项」，入口不会渲染——这也正是 `dsh web` 的浏览器标签页所看到的，因为那个表层会实时重读自己的 patch 层，本就没有需要重启的东西。

确认弹窗是无条件的，而不是以「会话是否繁忙」为条件。重启会停掉整个本机会话进程，而浏览器没有跨会话的在途视图：`SessionListState` 带有 ids、摘要、phase 与 jobs，却没有 running 标志。每次都问是诚实的默认；有条件的提示只能靠猜。弹窗写明了重启的代价——进行中的回答会被中断，已保存的记录不受影响。

该插件由壳自己的 patch 层激活（`apps/desktop/desktop.patch.yml`，以 `--patch` 传入），绝不经由共用的 web 组合包，因此 `npx @deepseek-ai/dsh web` 的组合与此前完全一致。

## Model Experience

无，因为该插件只贡献一个浏览器控件、没有 host 侧行为；这里没有任何东西会到达模型请求。

#### KV Cache effect

无；本包既不组装也不发送 provider 请求。

## Known Limitations and Deferred Work

- **只能重启，不能热载**：新装的插件无法挂进运行中的页面——客户端 HMR 接收端会拒绝不在其 loader tree 中的 entry，`window.__DSH_BOOT__` 是注入到所服务 index 中的，因此新 entry 需要一份新文档，而能重组 host 树的根 Include entry 又是 `dsh-app-boot` 的私有对象。三者中任何一条单独成立都足以否定热载。
- **提示无法区分繁忙与空闲的会话**：见上；它每次都问。
- **轮询而非推送**：壳没有通往该页面的事件通道，因此入口每五秒重问一次。变更会在这个窗口内可见，而不是立即可见。
