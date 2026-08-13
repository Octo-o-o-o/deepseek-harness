# Agent Note: 桌面 per-launch token 走 env 与 HttpOnly cookie

Status: implemented

[English](2026-08-14-desktop-per-launch-token.md) | 中文

## 问题

回环 `/api` 是可达性围栏，不是认证。把 per-launch token 放进 argv（`ps` / `/proc/cmdline`）或未认证的 index（`window.__DSH_TOKEN__`），本机任意进程都能偷走并调用 `/api`。

## 决策

壳注入成对的 `DSH_DESKTOP_TOKEN` 与 `DSH_DESKTOP_BOOTSTRAP_NONCE`。只设其中一个会让 web-startup 加载失败。index 只拿到 nonce（`JSON.stringify` 再转义角括号）。`POST /__dshd_bootstrap` 在 30s 内单次消费 nonce，并设置 `Set-Cookie: dsh-token=…; Path=/api; HttpOnly; SameSite=Strict`（`/__dshd_ready` 再写一条同名 cookie）。连接层 node 半仍接受 `X-DSH-Token` 或该 cookie，方便壳自检而不走 bootstrap。两条下行都建立后浏览器 POST `/__dshd_ready`；壳用 `X-DSH-Bootstrap` 轮询 `/__dshd_status` 再进入 `Visible`。没有这对环境变量则保持未经认证的 CLI 默认。

## 考虑过的替代方案

**保留 `--desktop-token`，只是不再打日志。** 同用户下每个进程仍能读 argv。

**经 Tauri IPC 把 token 交给 renderer。** JS 里仍是 bearer。HttpOnly cookie 加上壳只用 header 自检，让页面拿不到秘密。

## 后果

不带成对环境变量的 `dsh web` 行为不变。已消费或过期的 nonce 不能再换 cookie。桌面 Loader settle 后，owner 不是 `client-connection` 的 `/api` 注册会被拒绝。
