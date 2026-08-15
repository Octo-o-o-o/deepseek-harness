# DeepSeek Harness Desktop（dshd）

中文 | [English](README.en.md)

把 [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness) 装进一个双击就能用的桌面应用。**非官方社区项目，与 DeepSeek 无关联。**

[![下载 macOS](https://img.shields.io/badge/下载-macOS%20Apple%20Silicon-000?logo=apple)](https://github.com/Octo-o-o-o/deepseek-harness-desktop/releases/latest) [![下载 Windows](https://img.shields.io/badge/下载-Windows%20x64-0078D4?logo=windows)](https://github.com/Octo-o-o-o/deepseek-harness-desktop/releases/latest) [![官网](https://img.shields.io/badge/官网-dshd.octoooo.com-f0ede4)](https://dshd.octoooo.com)

## 向 DeepSeek 致敬

内核不是我写的。`dsh` 是 [DeepSeek AI](https://deepseek.com) 开源的 agent harness，以 **「一切皆插件」** 为架构，由 [Cordis](https://github.com/cordiverse/cordis) 驱动，其设计思想见论文[《一种面向时空可组合性的编程范式》](https://github.com/cordiverse/paper)。

真正难的部分——agent 循环、插件体系、会话事件溯源、工具与权限模型、Web GUI——全部来自官方项目，以 MIT 开源。本仓库**一行都没有改动它的内核逻辑**，只做了一件事：把它装进一个不需要终端、不需要安装 Node 的桌面壳里，并把签名、公证、分发这条链路跑通。

能有这样一个架构清晰、文档扎实、连事件契约都写得一丝不苟的开源 harness 可用，是这个项目存在的前提。感谢 DeepSeek 把它开源出来。

## 下载

| 平台 | 文件 | 说明 |
|---|---|---|
| macOS · Apple Silicon | `dshd-0.1.0-arm64.dmg` | Developer ID 签名 + Apple 公证 + staple，双击即开 |
| Windows · x64 | `dshd_0.1.0_x64-setup.exe` | NSIS 安装器，个人安装推荐 |
| Windows · x64 | `dshd_0.1.0_x64_en-US.msi` | MSI，用于批量部署 |

前往 [最新 Release](https://github.com/Octo-o-o-o/deepseek-harness-desktop/releases/latest) 或[官网](https://dshd.octoooo.com)下载。每个包都附 SHA-256，建议核对后再安装。

- **macOS 13.5+，仅 Apple Silicon**。Intel Mac 请用 `npx @deepseek-ai/dsh`。
- **Windows 10 / 11 x64**。没有代码签名证书，首次运行 SmartScreen 会拦一次，选「更多信息 → 仍要运行」。

## 这个桌面壳做了什么

不是把网页套个窗口就叫桌面版。下面每一条都是实际写进代码、并有测试或真机验证的：

**开箱即用**
- 自带钉死版本的 Node 运行时（v24.19.0），**不需要预装 Node**，也不碰你系统里已有的 Node。
- 应用启动时拉起本地 sidecar，只监听 `127.0.0.1` 的随机端口，就绪后才把 WebView 导航过去。托盘常驻，关窗不退出。
- 从 Dock/任务栏点击能恢复窗口，单实例锁避免开出第二个自己。

**数据与命令行实时互通**
- 数据目录就是 CLI 用的 `~/.dsh`，会话、设置、工作区**双向实时共享**——你在终端里 `npx @deepseek-ai/dsh` 开的会话，桌面端立刻能看到，反之亦然。
- 旧版桌面应用（用 app-data 目录）首次启动会迁移到 `~/.dsh`，且仅当 `~/.dsh` 尚无自身数据时才迁移，**绝不覆盖**已有的 CLI 数据；迁移失败会自动回滚。

**进程生命周期不留残骸**
- Windows 用 Job Object、Unix 用进程组来管理 sidecar 的**整棵进程树**：强杀应用不会留下孤儿 node 进程。
- 存活判定按进程组而非组长（组长退出不代表树已死），TERM 之后仍有幸存者才升级为 KILL。
- pid 文件记录进程创建时间，pid 复用时必然不匹配，因此绝不会误杀你自己启动的 `dsh web`。

**安全上的取舍是认真做的**
- 每次启动生成独立 token，`/api` 只认 HttpOnly cookie；**引导用的一次性 nonce 经 URL fragment 送达**——user agent 从不把 fragment 发上网络，所以扫本机端口的进程在页面响应里拿不到它（loopback 不携带用户身份，这条很关键）。
- WebView 有导航围栏：只放行内置起始页与本机 sidecar，模型回答里的外链一律交给系统默认浏览器，不会顶掉应用界面。
- 传给 sidecar 的环境变量是白名单，你 shell profile 里为别的服务导出的密钥不会流进 agent；`PATH` 例外，取自登录 shell，否则从 Dock 启动时 agent 的 bash 工具会找不到你装的工具链。
- 会话日志有单写者保护：桌面端与 CLI 同时写同一会话时会被明确拒绝并报错，而不是让日志静默损坏。

**发布链可复现**
- 载荷清单（`payload-manifest.json`）按平台分节记录 322/321 个外部依赖的解析版本，任何漂移都会让构建失败而不是悄悄产出不同的包。
- macOS 侧签名覆盖包内每一个 Mach-O 文件（按文件头识别，不靠可执行位），公证与 staple 后还会挂载 DMG 复验 Gatekeeper。

## 特意没有做的事

- **不改内核**。插件体系、Web UI、agent 循环全部原样使用官方代码。想用命令行或参与核心开发，请直接去[官方仓库](https://github.com/deepseek-ai/deepseek-harness)。
- **不做云端**。没有账号体系，没有遥测回传，应用数据和本地服务都跑在你自己的机器上。
- **不替你保管密钥**。API Key 沿用 harness 自己的凭据层（环境变量 / `~/.dsh/.credentials.yaml` / `.env`），桌面壳不额外收集。
- **暂不支持 Intel Mac**：包内 Node 运行时是 arm64。
- **Windows 尚无代码签名证书**，所以有 SmartScreen 提示；这不是能靠改代码绕过的事。

## 成熟度

内核处于官方的 **developer preview（rc）** 阶段，官方明确声明会有破坏兼容性的变更。桌面壳本身版本号还是 `0.1.0`。尝鲜很好，**请勿在其中存放重要数据**。

## 与官方项目的关系

本项目基于 [deepseek-ai/deepseek-harness](https://github.com/deepseek-ai/deepseek-harness) 构建：核心能力、插件系统和 Web UI 全部来自官方项目。本仓库只负责 Tauri 2 桌面封装、本地服务生命周期管理、托盘与窗口集成，以及安装包构建与签名分发。

本仓库是完整的 harness 代码，因此下面这些用法一样成立。

### 从 npm 运行

```sh
npx @deepseek-ai/dsh web
```

默认在 `http://127.0.0.1:3080` 启动 Web UI，见 [Web UI 指南](docs/user/guide/index.md)。

### 从源码运行

```sh
git clone https://github.com/Octo-o-o-o/deepseek-harness-desktop.git
cd deepseek-harness-desktop
pnpm install
pnpm run build
pnpm dsh web
```

桌面端的构建与设计见 [`apps/desktop`](apps/desktop/README.zh.md)，Windows 打包见 [WINDOWS-BUILD.zh.md](apps/desktop/WINDOWS-BUILD.zh.md)。

## 开发

先读[开发指南](docs/development.md)与[架构文档](docs/architecture.md)。agent 请遵循 [AGENTS.md](AGENTS.md)。

## 许可

[MIT](LICENSE)。原 DeepSeek Harness 仓库的代码与文档版权归 DeepSeek AI 所有。

第三方依赖及其许可见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
