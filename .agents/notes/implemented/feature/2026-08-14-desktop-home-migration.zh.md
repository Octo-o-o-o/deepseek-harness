# Agent Note: 桌面数据目录、首启迁移与进程锁

Status: implemented

[English](2026-08-14-desktop-home-migration.md) | 中文

## 问题

`dsh web` 默认写 `~/.dsh`。打包后的桌面应用若沿用该路径，会与 CLI 争同一写者，并把会话放在用户 shell 配置旁边。桌面还需要一次性拷贝已有 CLI 数据、拒绝第二个写者，以及 sidecar 崩溃后仍在的日志位置。

## 决策

壳注入的 `DSH_HOME` 在 macOS 为 `~/Library/Application Support/DeepSeekHarness`，在 Windows 为 `%APPDATA%\DeepSeekHarness`，可被环境变量覆盖。新目录没有 `migration-state.json` 时，首启从 `~/.dsh`（或 `DSH_LEGACY_HOME`）复制 `sessions`、`settings`、`attachments`、`storages`、`profiles`。凭据不复制。新目录里已有的文件先快照到 `migration-backup-<ts>`；`DSH_DESKTOP_MIGRATE_FAIL=1` 或任何拷贝错误会恢复该快照且不写 marker。`desktop.lock` 是排他 `flock`（Windows 为 `share_mode(0)`，未在本机验证）。`desktop-state.json` 记住 sidecar 的 cwd。`logs/sidecar.log` 在 50MB 时轮转；panic 追加到 `logs/crash.log`。

## 考虑过的替代方案

**与 CLI 共用 `~/.dsh`、不做迁移。** 只剩一棵树，但两个写者、以及把数据放进打包应用的隐藏点目录，已被桌面方案否决。

**移动而不是复制。** 移动失败会毁掉 CLI home；复制加 marker 会留下完整的 `~/.dsh`。

## 后果

第二个桌面进程看到中文的“数据目录正被占用”错误，不会再拉起 sidecar。测试覆盖复制、跳过凭据、注入回滚、锁争用、日志轮转和 `desktop-state.json`。错误页打开日志目录在 WebView 能 invoke 时走 Tauri 命令。
