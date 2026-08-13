# Agent Note: 桌面 per-launch token 在 CLI 上缺省无感，壳内必带

Status: implemented

[English](2026-08-14-desktop-per-launch-token.md) | 中文

## 问题

回环 `/api` 是可达性围栏，不是认证。本机任意进程都能 POST，也能打开两条下行 WebSocket。打包后的桌面应用必须挡住这一点，又不能改变 CLI 用户的 `dsh web`。

## 决策

`--desktop-token <tok>` 缺省为空：没有 flag 就不校验、不注入页面。有 token 时，`dsh-web-app` 通过 tapIndex 写入 `window.__DSH_TOKEN__`（转义，不进日志）。连接层对 `X-DSH-Token` 或 cookie `dsh-token` 做常数时间比较，失败则 401 / 拒绝 upgrade。桌面壳每次启动都生成 hex token、带上 flag，并在进入 `Visible` 之前探测 `POST /api/host.describe` 以及 `/api/events.mux` 与 `/api/events.host` 的升级。

## 考虑过的替代方案

**带 OS ACL 的 Unix domain socket / named pipe。** 更强，留到之后的 M1 评估。

**把 token 放进就绪行 URL。** 会进日志和进程列表。

## 后果

不带 flag 时，既有 CLI 和 `test:gui` 行为不变。带 flag 时，无头访问 `/api` 为 401。token 不进 URL，也不写 sidecar 日志。
