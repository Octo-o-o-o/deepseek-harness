# Agent Note：桌面 WebView 把链接挡在外、把拖入的文件放进来

Status: implemented

[English](2026-08-14-desktop-webview-navigation-rules.md) | 中文

## Problem

主窗口完全声明在 `tauri.conf.json` 里，因此没有任何导航处理器，默认值带来三个行为。

模型回答里的链接点了没反应。markdown 渲染给外链标 `target="_blank"`；WebKit 会向 UI delegate 请求新窗口，而未安装 `on_new_window` 处理器时 `wry` 返回空。

普通外链会顶掉整个应用。没有 `on_navigation`，任何导航都加载进主窗口，而它既没有地址栏也没有后退入口。

拖进对话的文件没反应。`dragDropEnabled` 默认开启，`tauri-runtime-wry` 安装的处理器对每个拖放事件都返回 `true`，`wry` 因此从不回落到 WebKit 自身的处理，输入框的 `drop` 监听器永远收不到事件。

## Decision

窗口条目的几何参数仍留在 `tauri.conf.json`，加上 `"create": false`，由 `build_main_window` 依该条目创建，处理器才挂得上去。

`navigation::is_internal_url` 按字面主机名放行内置起始页与回环 sidecar；`localhost` 与 `[::1]` 一律拒绝，因为它们可能解析到并非本壳拉起的监听端。其余全部拒绝并交给 `opener::open_external_url`，后者只启动 `http` 与 `https`——被拒的导航携带的是页面内容给的 URL，若启动 `file:` 或某个应用私有协议，就等于让页面内容借本壳之手够到磁盘或另一个应用。

`dragDropEnabled` 关闭，页面自己的 drop 事件随之恢复。

## Alternatives considered

**保留 Tauri 的拖放处理器，把路径经 IPC 转发给页面。** capability 并不覆盖 sidecar 的远程源，页面收不到，除非扩大权限面。

**引入 `tauri-plugin-opener`。** 本壳已经会为日志目录拉起文件管理器；为多一次拉起引入插件，要多一条权限、一份生成的 ACL 和一条许可证记录，而实际代码只有十五行左右。

## Consequences

链接在浏览器里打开，应用窗口始终停在应用上。单测覆盖内部 URL 规则与被拒的协议。已实测：在启动守护进程环境下启动可加载到 sidecar 页面——WebKit 的网络进程持有三条到 sidecar 端口的连接。点击链接属于人工检查。
