# DeepSeek Harness

[English](README.md) | 中文

DeepSeek Harness（`dsh`）是由 [DeepSeek AI](https://deepseek.com) 开发的开源 agent harness（智能体框架）。

它采用**一切皆插件**的架构，并由 [Cordis](https://github.com/cordiverse/cordis) 驱动，其设计参见论文 [_A Programming Paradigm for Spatiotemporal Composability_](https://github.com/cordiverse/paper)。

## 开发者预览

DeepSeek Harness 目前处于 _开发者预览_ 阶段，正在快速迭代。**未来将出现破坏兼容性的变更。**

## 运行

### 通过 `npm` 运行

安装 `Node.js`，然后运行：

```sh
npx @deepseek-ai/dsh web
```

该命令会启动 Web UI，默认地址为 `http://127.0.0.1:3080`。详见 [Web UI 指南](docs/user/guide/index.md)。

### 桌面版（dshd）

习惯双击安装、不想碰终端的用户可以使用原生桌面版 **dshd**（macOS，Apple 芯片）。它把完整的 dsh Web UI、固定版本的 Node 运行时与本地回环服务打包进一个**独立自足**的应用——无需安装 Node.js，也不需要单独启动前后端。

- 下载：[最新 GitHub Release](https://github.com/Octo-o-o-o/deepseek-harness/releases/latest)（`dshd-*.dmg`，已通过 Apple 公证）。
- 安装：打开 DMG，把 `dshd` 拖进「应用程序」。
- 应用已签名并公证（Developer ID），双击即可打开；可用随附的 SHA256 校验完整性。
- Windows 构建与完整桌面设计见 [`apps/desktop`](apps/desktop/README.md) 与 [`proposals/desktop-gui-ecosystem.md`](proposals/desktop-gui-ecosystem.md)。

应用使用 `~/.dsh` 作为数据目录——与 npm CLI 版**同一个目录**,因此你之前在 `npx @deepseek-ai/dsh web` 里的会话、设置与工作区会**直接出现**,并且两个方向实时共享。

- 与 npm 版同时运行:两个进程各自监听不同的本机端口、互不阻塞,但会**并发写同一份会话数据**,请避免同时运行两者;dshd 自身带有目录锁防止多开,CLI 暂不持锁。
- 旧版 dshd(应用数据目录)会在首次启动时迁移到 `~/.dsh`——仅当 `~/.dsh` 里还没有你自己的会话数据时才会执行,绝不覆盖。

### 从源码运行

如需从仓库源码运行：

```sh
git clone https://github.com/deepseek-ai/deepseek-harness.git
cd deepseek-harness
pnpm install
pnpm run build
pnpm dsh web
```

## 社区与支持

- 欢迎通过 [GitHub Discussions](https://github.com/deepseek-ai/deepseek-harness/discussions) 提交反馈或 bug 报告。
- 为你的插件仓库添加 [`dsh-plugin`](https://github.com/topics/dsh-plugin) 话题，便于被发现。
- 欢迎加入 DeepSeek Harness 企微群：扫码添加企微小助手并填写入群问卷，完成后小助手会邀请你入群。

<table>
  <thead>
    <tr>
      <th align="center">企微小助手</th>
      <th align="center">入群问卷</th>
      <th align="center">微信公众号</th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td align="center"><img src="assets/community-wecom-assistant.png" alt="DeepSeek Harness 企微小助手二维码" width="180" height="180"></td>
      <td align="center"><a href="https://trtgsjkv6r.feishu.cn/share/base/form/shrcnIt5twSVdLGD52KJBckGCgg"><img src="assets/community-wecom-survey.png" alt="DeepSeek Harness 入群问卷二维码" width="180" height="180"></a></td>
      <td align="center"><img src="assets/community-wechat-official-account.png" alt="DeepSeek Harness 团队微信公众号二维码" width="180" height="180"></td>
    </tr>
  </tbody>
</table>

## 参与贡献

参见 [CONTRIBUTING.md](CONTRIBUTING.md)。

## 开发

请先阅读[开发指南](docs/development.md)与[架构文档](docs/architecture.md)。

面向 agent：请遵循 [AGENTS.md](AGENTS.md)。

## 许可证

[MIT](LICENSE)

第三方依赖及其许可证见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
