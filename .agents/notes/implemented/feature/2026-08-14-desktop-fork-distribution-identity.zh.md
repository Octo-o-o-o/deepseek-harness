# Agent Note：桌面构建使用本 fork 自己的发行标识

Status: implemented

[English](2026-08-14-desktop-fork-distribution-identity.md) | 中文

## Problem

桌面应用此前以 `com.deepseek-ai.dsh.desktop` 发行，把 DeepSeek 写作 publisher，并打开一个标题为 `DeepSeek Harness Desktop` 的窗口。本 fork 是上游 Web 界面的社区桌面部署，而不是 DeepSeek 的产品（[下游部署](../architecture/2026-08-14-desktop-shell-downstream-deployment.md)），且它由本 fork 自己的 Developer ID 签名，而非 DeepSeek 的。

bundle identifier 是 macOS 在 launch services、defaults 与 keychain 中解析应用所用的名字，因此一个上游口径的 identifier 会让社区构建与一个假想的官方构建在这些注册表里相撞。publisher 与窗口标题则做出了产物本身支撑不了的来源声明。

## Decision

bundle identifier 为 `com.octoooo.dshd`，publisher 是签名并发布它的账号，shortDescription 写明 `Unofficial`，窗口标题为 `DeepSeek Harness Desktop (Unofficial)`。

免责声明只出现一处，在窗口标题里，而不是铺满每个界面。应用在，标题就在；它是窗口管理器上报的那个字符串；而一处清楚的声明会被读到，重复的那种会被略过。侧栏继续称呼它运行的产品，因为那个名字是准确的：这就是 DeepSeek Harness 的 Web 客户端。

运行时没有任何变化。数据目录仍是 `$DSH_HOME`，默认 `~/.dsh`，它由主目录而非 bundle identifier 推导，因此会话、设置与工作区不受本次改名影响，也继续与 npm 版 CLI 共享。

## Alternatives considered

**沿用上游 identifier。** 这能保留从本 fork 早前构建原地升级的路径，同时把一个社区二进制注册在一个指向他方的 identifier 之下，而这正是 identifier 存在所要避免的相撞。

**把免责声明放进产品名。** `dshd (Unofficial)` 会进入 Dock、应用程序文件夹与每一个文件对话框。这是在每一份被阅读的列表里长期支付的代价，换取一个只需知道一次的事实。

**加一个 About 面板来承载它。** macOS 会用 bundle 元数据生成一个，但要看到它需要一个本应用原本不需要的菜单，于是这条声明会既真实又无人阅读。

## Consequences

macOS 按 identifier 解析应用，因此本次构建是一个新应用，而不是对从早前 Release 安装的那个的升级：旧的 `dshd` 会一直留到被删除，而 Launch Services、保存的窗口状态以及任何按应用授予的权限都从空开始。用户数据不受影响，因为它从未存放在 identifier 之下。

npm 版 CLI 的 `dsh web` 不受触及；此处描述的标识只属于打包后的桌面应用。
