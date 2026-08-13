# Agent Note: 桌面侧车打包 = pnpm deploy + 钉死的 Node 运行时

Status: implemented

[English](2026-08-14-desktop-sidecar-pack.md) | 中文

## 问题

桌面壳在开发态可以拉起 checkout 里的 `dsh web`，但发出去的 `.app` 不能假设 PATH 上有 `node` 或 `pnpm`。`@deepseek-ai/dsh` 的 `pnpm deploy --prod` 是 CLI 已有的安装形态；这棵树单独拿出来并不是 Node 能完整看见的闭包。应用的直接依赖被提升成符号链接，Service Definition 包只以 peer 存在，而从 realpath 后的包发出的 ESM 导入看不见 `$DSH_HOME/profiles/node_modules`。

## 决策

`apps/desktop/scripts/pack-sidecar.mjs` 分三步生成 `sidecar/dist`：`pnpm --filter @deepseek-ai/dsh deploy --prod --legacy`、按宿主 triple 下载并做 sha256 校验的 Node v24.19.0 运行时，以及一条剥掉 PATH 的启动自检（必须打出就绪行、`GET /` 含 `__DSH_BOOT__`、SIGTERM 后退出）。脚本再把 `.pnpm` 里每个包提升进 `app/node_modules`，让逐级向上查找与工作区 checkout 所见一致。`apps/cli` 列出 `pnpm deploy --prod` 否则会漏掉的 Service Definition 包（`dsh-timeout`、`dsh-invariants`、`dsh-subprocess` 以及同一组 peer），因为 CLI 是组装面。Tauri `bundle.resources` 把 `sidecar/dist/bin/node` 映到 `bin/node`、把 `sidecar/dist/app` 映到 `app`，但复制会丢掉目录符号链接，所以 `pack-sidecar.mjs embed` 在 `tauri build` 之后用 `cp -a` 再拷一遍自检过的树。壳在 `.app/Contents/Resources` 下定位 `bin/node` 和 `app/lib/bin.js`。启动时要把隔离 store 摊平进 `$DSH_HOME/profiles/node_modules`，还依赖 [heal 的 realpath 查找](../bug-fix/2026-08-14-heal-follows-hoisted-symlink-realpath.md)。

## 考虑过的替代方案

**只依赖 PATH 上的 `node` 和 checkout 的 `apps/cli/lib/bin.js`。** 这是 M0 开发路径，过不了干净机器自检。

**把 `.pnpm` store 拷成真实目录来拍平。** 正确，但体积翻倍；提升后的符号链接只留一份，并与 Node 跟随符号链接的行为一致。

**把缺的 Service Definition 写到 `@deepseek-ai/dsh-base` 而不是 CLI。** bundle 已经列出行插件；可 deploy 的 `dsh` 组装面按任务要求是 `apps/cli`。

## 后果

剥掉 PATH 后运行 `sidecar/dist/bin/node sidecar/dist/app/lib/bin.js web --port 0 --host 127.0.0.1` 是打包门禁。`.app` 未签名；公证不在本次变更内。Windows 上解 Node zip 留到桌面 CI 车道。
