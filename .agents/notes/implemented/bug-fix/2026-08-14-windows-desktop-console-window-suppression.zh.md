# Agent Note: 抑制桌面壳在 Windows 上弹出的控制台窗口

Status: implemented

[English](2026-08-14-windows-desktop-console-window-suppression.md) | 中文

## Problem

release 壳以 GUI 子系统运行（`main.rs` 的 `windows_subsystem = "windows"`），自身没有控制台。用 `Command::spawn` 启动控制台子系统的 sidecar `node.exe` 时不加创建标志，Windows 会为子进程新分配一个控制台，安装后的应用每次启动都会在主窗口旁边弹出这个控制台窗口。孤儿 sidecar 回收路径（`taskkill /PID … /T /F`）触发时也会闪出同类窗口。

## Decision

`process.rs` 提供 `hide_child_console(&mut Command)`：Windows 上经 `std::os::windows::process::CommandExt` 应用 `CREATE_NO_WINDOW` 创建标志（`0x0800_0000`），其他平台为空操作。sidecar spawn 与 `taskkill` 回收两处调用它；`ps` 身份探测编译为 `#[cfg(not(windows))]`，无需处理。sidecar 一处加标志即覆盖整棵树：`dsh-subprocess-local` 在 Windows 上从不设置 `detached`，bash/pwsh 及所有孙进程继承 sidecar 的无窗控制台，而不是各自新开。

## Alternatives considered

**用 `DETACHED_PROCESS` 取代 `CREATE_NO_WINDOW`。** 否决：它让子进程完全没有控制台，需要控制台的孙进程只能新开一个（可见的）控制台；`CREATE_NO_WINDOW` 保留一个可被整棵树继承的隐藏控制台，且正是 Node 自身 `windowsHide` 映射的标志。

**在 Node 侧每个 spawn 加 `windowsHide: true`。** 否决为冗余：Windows 上非 detached 子进程继承父控制台，藏住 sidecar 的控制台即藏住整棵树；既有的 `windowsHide` 调用点（`dsh-directory-picker-native`、`dsh-native-command`）覆盖的是父进程可能持有可见控制台的 spawn。

**只修 sidecar spawn。** 否决：崩溃后回收孤儿 sidecar 时仍会闪出控制台窗口。

## Consequences

安装版 Windows 应用启动与孤儿回收都不再弹出终端窗口。debug 构建保留自己的控制台，且无可观察损失：sidecar stdio 本就通过管道写入 `$DSH_HOME/logs/sidecar.log`，从不进父控制台。一个 Windows-only 测试守护标志值始终是合法的创建标志（非法值会让所有子进程 spawn 失败），另一个守护 `taskkill` 在该标志下仍能终结目标 pid。
