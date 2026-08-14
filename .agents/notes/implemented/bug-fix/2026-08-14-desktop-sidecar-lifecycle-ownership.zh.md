# Agent Note：桌面关停按进程组判定，且主线程不再 join

Status: implemented

[English](2026-08-14-desktop-sidecar-lifecycle-ownership.md) | 中文

## Problem

[supervisor 那轮改造](../feature/2026-08-14-desktop-sidecar-supervisor.md)之后仍有四个生命周期缺陷。

`ChildTree::is_alive` 只问 leader。sidecar 会在自己的进程组里拉起 bash、pwsh 与选择器进程，因此一个收到 SIGTERM 就退出的 leader 会让整棵树看起来已死：强制 kill 被跳过，随后对日志 reader 的无上限 join 会一直等在仍被孙进程持有的管道端上。退出因此挂起。

`RunEvent::Exit` 先 `request_stop()` 再 join boot 线程——而它跑在主线程上。boot 线程轮询 `window.url()`，该调用向同一个事件循环投递消息并阻塞等待回复。两者互等。

`SidecarSupervisor::install` 在 mutex 之外读 `stopping`。若一次停止恰好落在该读取与写入之间，它会取走空槽位并返回，之后 `install` 存进去的进程再没有人负责停止。

启动完成后没有任何东西看着 sidecar。它退出后，窗口仍显示着一个服务器已经消失的页面。

## Decision

`is_alive` 先回收 leader，再用 `killpg(pgid, 0)` 询问整个组，只有 `ESRCH` 才算消失。join 日志 reader 最多等 2s，超时则让线程处于分离状态——退出不能依赖某个孙进程关闭管道。

`RunEvent::Exit` 只停止并返回；`AppState::join_boot` 删除。`install` 与 `request_stop` 都在持有 mutex 的前提下读写 `stopping`，并且都在锁外执行关停。

启动序列到达 `Visible` 之后，boot 线程停在 `wait_for_unexpected_exit` 上。无人请求的退出会停止本壳、返回启动错误，而 `show_error` 会先导航回内置起始页再写入消息——此时窗口位于 sidecar 的源上，而那个源正是刚刚死掉的东西。

## Alternatives considered

**单独起一个看门狗线程。** 它会与停止路径争夺子进程的所有权；boot 线程本就拥有整个序列，且在 `Visible` 之后无事可做。

**自动重启。** `boot_and_navigate` 会安装 panic hook、取目录锁并执行迁移，因此不可重入。原地重启需要一个带上限的状态机，只有在错误态存在之后才值得做。

## Consequences

已在启动守护进程环境下实测：`kill -9` sidecar 后 2s 内 `sidecar.pid` 被清除，应用仍在运行且无孤儿进程；对壳发 SIGTERM 约 1s 退出，无残留。
