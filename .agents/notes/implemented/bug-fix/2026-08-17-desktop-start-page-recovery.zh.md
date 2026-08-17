# Agent Note: 启动页能拉起已死 sidecar，并拒绝对仍存活会话的手势返回

Status: implemented

[English](2026-08-17-desktop-start-page-recovery.md) | 中文

## Problem

sidecar 退出或启动失败后，启动页只展示日志尾和「打开日志目录」。回到正在跑的 host 的唯一办法是从托盘 Quit 再开。另外，WKWebView 历史可以在 sidecar 仍活着时回到打包启动页（`tauri://localhost` 或 `http://tauri.localhost`）。该页会永远停在 “Starting the local host…”，因为它的 splash 钩子不会再次导航。

## Decision

启动页错误态提供 **Restart local host**，调用已有的 `restart_for_plugins` 命令。默认 capability 现在把该命令授给主窗口的启动页；sidecar origin 仍然只通过每次启动的 remote capability 拿到它。整进程重启会释放 home 锁并重新拉起 sidecar。

WebView 一导航到 sidecar，`AppState` 就保存不含 bootstrap nonce fragment 的回环 origin——写在 client-ready 等待之前，这样那段窗口里的手势返回也不能把 splash 拉回来。只要该 origin 还在，`on_navigation` 就拒绝启动页 URL 并导航回去。`request_stop` 清掉 origin，这样真正的失败仍能回到启动页。更新 `install` 在这次 stop 之后失败时，同样导航回启动页并显示 Restart。

## Alternatives considered

**在同一进程里重入 `boot_and_navigate`。** 这次改动否决：boot 线程、home 锁和 supervisor 都按单次通过来写。进程重启是插件重启路径已经拥有的恢复方式。

**把手势返回写成已知限制。** 否决：启动页会谎称 host 还在启动，而它其实已经在跑。

**带着原来的 `#dshd-nonce=` fragment 再导航。** nonce 只能用一次。bootstrap 之后 cookie 已足够；保存的 origin 是 `http://127.0.0.1:<port>/`。

## Consequences

启动失败或 sidecar 意外退出时，可以从启动页恢复，不必托盘 Quit。仍存活的会话不能被历史后退换成 splash。重启仍会杀掉进行中的 turn，与插件重启相同。

## Testing

`cargo test` 覆盖 `is_start_page` 与回环 sidecar 的区分、updater `install_verified_update` 的顺序，以及 install 失败时 sidecar 已经停下。未覆盖：打包 WebView 里的 GUI 手势返回、在真实错误 splash 上点击 Restart，以及 `install()` 失败后回到该 splash。
