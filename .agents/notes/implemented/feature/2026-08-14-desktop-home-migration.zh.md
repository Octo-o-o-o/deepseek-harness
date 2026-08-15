# Agent Note: 桌面数据目录、首启迁移与进程锁

Status: implemented

[English](2026-08-14-desktop-home-migration.md) | 中文

## 问题

`dsh web` 默认写 `~/.dsh`。早前的桌面构建为避免两个写者，给壳单独分配了平台 app-data 目录；这把用户的会话、设置与工作区拆成两棵永不汇合的树，同样的工作会因入口不同而呈现不同面貌。桌面仍然需要从那个被废弃的目录做一次性拷贝、拒绝第二个桌面写者，以及 sidecar 崩溃后仍在的日志位置。

## 决策

壳注入的 `DSH_HOME` 就是 CLI 自己的 `~/.dsh`，可被环境变量覆盖；桌面窗口与 `npx @deepseek-ai/dsh web` 因此读写同一棵树，并可同时运行，各自使用系统分配的端口。

首启只在 `~/.dsh` 既没有 `migration-state.json`、**也**没有那些目录时，才从统一前的桌面目录（`~/Library/Application Support/DeepSeekHarness`、`%APPDATA%\DeepSeekHarness` 或 `DSH_LEGACY_HOME`）复制 `sessions`、`settings`、`attachments`、`storages`、`profiles`：已有真实 CLI 数据的目录具有权威性，绝不被覆盖。凭据不复制。目录里已有的文件先快照到 `migration-backup-<ts>`；`DSH_DESKTOP_MIGRATE_FAIL=1` 或任何拷贝错误会恢复该快照且不写 marker。

`desktop.lock` 是仅由壳持有的排他 `flock`（Windows 为全共享打开加 `LockFileEx` 字节范围锁，已在真实 Windows 主机验证），因此它排斥第二个 `dshd`，而不排斥 CLI 服务。`sidecar.pid` 同时记录 sidecar 的进程号**与**启动它的入口脚本，下次启动只在存活进程的命令行仍指向该脚本时才回收它：在共享目录里，仅凭进程号会在 pid 复用后授权杀死用户自己启动的 `dsh web`。`desktop-state.json` 记住 sidecar 的 cwd。`logs/sidecar.log` 在 50MB 时轮转；panic 追加到 `logs/crash.log`。

## 考虑过的替代方案

**保留独立的平台 app-data 目录。** 它从构造上消除了并发写者，但两棵树永不汇合，产品也没有在它们之间搬运工作的方案。

**从旧目录移动而不是复制。** 移动失败会毁掉数据；复制加 marker 会完整保留旧树以便手动重试。

**让 CLI 与桌面串行争同一个 home 锁。** 先启动者获胜、后启动者拒绝启动，而这恰恰是统一目录要提供的共存能力的反面。

## 后果

第二个桌面进程看到中文的"数据目录正被占用"错误，不会再拉起 sidecar；CLI 的 `dsh web` 不受影响并实时共享会话。测试覆盖复制、跳过凭据、拒绝覆盖已有产品数据、注入回滚、锁争用、日志轮转、`desktop-state.json`，以及记录 pid 上出现外来进程时拒绝回收。错误页打开日志目录在 WebView 能 invoke 时走 Tauri 命令。
