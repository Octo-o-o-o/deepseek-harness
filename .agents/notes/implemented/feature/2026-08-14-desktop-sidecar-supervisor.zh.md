# Agent Note: 桌面 sidecar 只有一个锁外停止入口

Status: implemented

[English](2026-08-14-desktop-sidecar-supervisor.md) | 中文

## 问题

Stage A 的壳会从托盘、ctrlc、`RunEvent::Exit` 和 `Drop` 同时停 sidecar。每条路径都握着 `Mutex` 调用 `shutdown`，第二次停止会在 5 秒宽限里堵在同一把锁上。`LoggingLines` 还会把就绪行之后的 stdout 写两遍。壳被 `SIGKILL` 后留下孤儿 Node 树，下次启动没有 pid 文件可收。

## 决策

`SidecarSupervisor::request_stop` 是唯一对外停止入口。它置取消、把 `SidecarProcess` 从 mutex 里取出，再关停。第二次调用是空操作。`SidecarProcess::shutdown` 幂等；`Drop` 只在没人关过时兜底。进程树 reap 之后再 join reader 线程。启动在各步检查 `is_cancelled`，导航失败回滚。`sidecar.pid` 记录活着的 Node pid；下次启动会对仍像 `dsh web` 的残留发 TERM/KILL。

## 考虑过的替代方案

**保留各处 shutdown，只在 `SidecarProcess` 上加重入标志。** 仍然有四个调用点，并且还在持锁等待。

**单独线程加 channel 做监督。** 正确，但比把进程从 mutex 里拿出来更重。

## 后果

托盘、ctrlc 和 Exit 共用一条路径。测试覆盖幂等停止、就绪行垃圾、HTTP 4MB 上限和 pid 文件记账。Windows Job Object 指派在本机仍未验证。
