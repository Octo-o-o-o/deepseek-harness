# Agent Note：桌面 sidecar 的 PATH 取自登录 shell

Status: implemented

[English](2026-08-14-desktop-login-shell-environment.md) | 中文

## Problem

从 Dock、访达或聚焦打开的应用继承的是启动守护进程的环境。读取已安装 `dshd.app` 下 sidecar 的环境，得到 `PATH=/usr/bin:/bin:/usr/sbin:/sbin`，没有 `SSH_AUTH_SOCK`，也没有 `LANG`。`env.rs` 的白名单如实照抄，而 `subprocess-local` 又以 sidecar 自身的环境组装每个工具子进程，于是 agent 的 `bash` 工具看不到 Homebrew、nvm、`cargo` 以及用户装的任何东西，走 SSH 的 `git push` 没有 agent socket，在 shell 配置里导出的 `DEEPSEEK_API_KEY` 也不存在——而同一套 harness 从终端启动时这些全都在。随包发布的 `rg` 与 `/usr/bin/git` 让搜索和多数版本控制照常工作，掩盖了差距有多大。

## Decision

`shell_env.rs` 每次启动执行一次 `$SHELL -ilc`，读取自带标记之间的 `env -0` 块：关闭 `stdin`、5s 期限、并设置 `DSH_RESOLVING_ENVIRONMENT=1`，让 shell 配置可以跳过只为交互会话准备的工作。shell 不存在、失败或超时则返回空，保留启动环境。

shell 的回答是取值来源，不是授权。`INHERITED_ENV` 仍是唯一闸门，因此 `.zshrc` 中为无关服务导出的凭据依旧进不了 agent。在该白名单之内，`PATH` 取登录 shell 的值，其余名字在有启动值时保留启动值：来自启动守护进程的 `PATH` 不是任何人的选择，而设置在应用进程上的变量是启动者设的。白名单新增 `SSH_AUTH_SOCK`、`SHELL`、`USER`、`LOGNAME`，以及自建证书颁发机构的网络所需的证书类名字（`NODE_EXTRA_CA_CERTS`、`SSL_CERT_FILE`、`SSL_CERT_DIR`）。

`DSH_DESKTOP_SHELL_ENV=0` 跳过探测。

## Alternatives considered

**改为让工具经登录 shell 运行**（`bash-local` 里用 `bash -lc`）。那是为一个桌面启动问题改动所有 harness 部署，而且每次工具调用都要重跑用户的 shell 配置。

**整体继承登录 shell。** 这会重新打开白名单本就是为堵住的泄漏：能读自身环境的 agent 就能读到用户导出的每一份凭据。

**仅当 `PATH` 等于启动守护进程默认值时才解析。** 它能让终端启动跳过探测，但把环境取值系在一次值比较上，`PATH` 恰好很短的用户会以难以预料的方式落空。

## Consequences

启动多付一次 shell 启动开销，上限 5s。单测覆盖标记解析、含 `=` 与换行的取值、未闭合的块、关闭开关、shell 缺失，以及合并规则（含白名单之外的名字无法进入）。已实测：在 `env -i` 加启动守护进程 `PATH` 下启动，sidecar 拿到的是用户完整的登录 `PATH`。
