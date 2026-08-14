# Agent Note：dshd 是已交付 Web 界面的下游桌面部署

Status: implemented

[English](2026-08-14-desktop-shell-downstream-deployment.md) | 中文

## Problem

上游交付两个应用装配——`apps/cli` 与 `apps/web`——没有打包应用。要用上交互产品，需要 Node、一个终端，以及 `npx @deepseek-ai/dsh web`；README 声明本项目处于 developer preview，并且会发生破坏兼容性的变更。

仓库还移除了自己的终端前端。`10bb9cbf4a`（2026-08-04）删除了 TUI 包与旧的 `dsh` 入口，共 28905 行。理由有记录，且是一个维护与清单诚实性的论证，而不是对用户的判断：隐式终端应用被移除之后，该包不再有任何已交付的组装，其仅存的消费者是项目生成器，继续保留它「使仓库所支持的应用清单产生误导」（[移除决策](../simplification/2026-08-04-remove-tui-package.md)）。该 note 的结论是 Web 成为已交付的交互界面，并写明了一个新前端必须具备什么：一个具名产品或部署、一条明确的包边界、一个具体的交互 provider，以及成套的生命周期与 transcript 验收。

因此本 fork 要填的不是缺失的界面——Web 客户端本身就是那个界面——而是缺失的「不用 Node 工具链和命令行也能装上并启动它」的方式。

## Decision

`dshd` 是既有 Web 界面的下游部署，不是把桌面前端推进上游的提案。它在 loopback 上监督一个 `dsh web` sidecar，加载该 sidecar 本就在提供的客户端；进程生命周期、数据目录与 token 规则由各自的 note 拥有（[supervisor](../feature/2026-08-14-desktop-sidecar-supervisor.md)、[home 迁移](../feature/2026-08-14-desktop-home-migration.md)、[per-launch token](../feature/2026-08-14-desktop-per-launch-token.md)）。

移除决策提出的那些条件，正是本部署所回应的。`dshd` 是一个具名产品，带有已签名并公证的 macOS 产物（[发布链](../process/2026-08-14-desktop-mac-signed-release.md)）；包边界明确落在 `apps/desktop`；它是一个具体的 supervisor 而非可复用的前端抽象；并具备成套验收：打包自检要求 sidecar 的就绪行、一次携带 `__DSH_BOOT__` 的 HTTP 200，以及在被剥离的 `PATH` 下干净的 `SIGTERM` 退出。

留在下游是本决策的用意，而非一种限制。移除决策所拒绝承担的成本——一个产品规模的前端，在这里还要加上签名、公证与安装包格式这条平台发布链——落在本 fork 身上。上游的应用清单继续如实描述上游所维护的东西。

developer preview 的立场被保留而非被违背。`dshd` 不新增任何协议字段、客户端能力或兼容承诺；它打包的是同一个 sidecar 提供的同一个客户端，因此上游的一次破坏性变更对 `dshd` 用户的影响，与对 `npx` 用户完全相同。本 fork 的立场是：preview 是把风险讲清楚的理由，而不是把产品挡在工具链之后的理由——一个人们可以试用、可以反馈、可以一同成长的不完善产品，胜过一扇关着的门。

外壳由 Tauri 2 承载。Host 本来就是一个独立的 Node 进程，因此 Electron 外壳会为系统 WebView 已经提供的窗口装饰多带一个浏览器引擎，而 sidecar 所需的 Node 运行时无论如何都要带。代价是明确的，并由本 fork 支付：Rust 进入一个 TypeScript 与 Node 的仓库，CI 需要 Rust 工具链，`cargo fmt`、`cargo clippy`、`cargo test` 成为门禁。第一张账单已经可见——Windows 的进程树终止与独占目录锁能够编译，但无法在构建机上验证，因此该信号由 CI 拥有。

## Alternatives considered

**把桌面应用贡献到上游。** 移除决策的成本论证原样适用，而一个打包应用还会额外带来一条平台发布链，这是一个 developer preview 阶段的 harness 当前没有理由承担的。fork 可以承担它；日后上游若想要其中任何一部分，那时再提供。

**用 Electron 构建外壳。** 它让整个仓库保持单一语言，且其发布工具链更成熟——一个平行的社区 fork 用它产出了已签名并公证的 DMG。因载荷被否决：多出来的引擎买到的是系统 WebView 已提供的窗口装饰，而 Node sidecar 两种方案都要带。被接受的代价是一条 Rust 工具链。

**改为重新引入 TUI。** 移除决策的门槛对终端前端与对本前端同样适用；而且终端前端仍然把终端留在用户的路径上，而这正是本部署要移除的那一步。

**在既有服务器上提供浏览器快捷方式或 PWA。** 因其仍然需要已安装的 Node 与一个手动启动的服务器而被否决。安装与启动这一步正是本决策的全部主题。

**等上游稳定之后再打包。** 作为本 fork 的产品判断被否决：风险是可以披露的，产物是可以替换的，而一个根本启动不了产品的人提供不了任何反馈。

## Consequences

一个人安装一个产物、双击即可使用；又因为数据目录就是 CLI 自己的 `~/.dsh`，会话与设置与 `npx @deepseek-ai/dsh web` 共享而非各存一份。

本 fork 拥有平台发布链、Rust 工具链、各平台 WebView 差异，以及跟踪上游破坏性变更的工作。由于 `dshd` 没有给客户端或协议增加任何能力，上游未来关于桌面界面的决策不受此处任何决定的约束。

以上关于上游的推断，仅限于仓库自身所陈述的内容——移除决策中的维护与清单论证，以及 README 中的 developer preview 声明。除这些记录之外，本 note 不对 DeepSeek 的意图作任何主张。
