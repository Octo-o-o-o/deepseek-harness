# Agent Note：桌面载荷记录自己装了什么，且不执行安装脚本

Status: implemented

[English](2026-08-14-desktop-payload-provenance.md) | 中文

## Problem

[sidecar 打包](2026-08-14-desktop-sidecar-pack.md)在打包时用 `npm install` 解析外部依赖，而这些依赖是 `commander ^15.0.0` 这样的范围。同一个 commit 在两天分别打包可能发出不同的代码，而没有任何东西比较这两者。该安装还会在持有 Developer ID 身份的机器上、以那台机器的权限执行依赖的 lifecycle script。

Node 运行时校验用的 `SHASUMS256.txt` 与产物同源同时下载，这只能确认传输而非发布者；被中断的下载还会在最终缓存路径上留下一个短文件，此后每次运行都失败，直到有人手工删除。

桌面版本声明分散在三处，没有任何东西比较它们；产物也没有记录内嵌 CLI 的版本。

## Decision

改为 `npm install --ignore-scripts`，再由 `restorePrebuildHelpers` 把 `prebuilds` 内 Mach-O 辅助程序的可执行位补回来——这本是安装脚本会做的事。本载荷需要的是 `node-pty` 的 `spawn-helper`：缺了它，包能加载，但每个终端都会以 `posix_spawnp failed` 失败。

`payload-manifest.json` 记录每个外部包解析到的版本、CLI 版本与 Node 版本。解析结果不同的打包会失败并指出差异；`pack-sidecar.mjs manifest` 用于有意记录新的解析结果。

`NODE_DIGESTS` 把各归档的 SHA-256 固定在仓库里。下载先落到临时名，校验通过才改名；缓存中校验失败的归档会重新下载，而不是卡住构建。

`assertDesktopVersion` 拒绝 `package.json`、`tauri.conf.json`、`Cargo.toml` 版本不一致的打包。

打包自检会用内嵌 Node 打开一个伪终端，并加载 `sharp`、`koffi` 与运行时自带的 SQLite。启动 Web 服务证明不了其中任何一项，而这恰恰是为另一种架构构建、或在无脚本条件下安装时会出错的地方。

## Alternatives considered

**用 `npm ci` 配合入仓的 lockfile。** 载荷自身的包是每次打包都从检出重建的 `file:` tarball，npm 会把它们的 integrity 写进 lockfile，因此源码一改 lock 就过期。

**给允许执行脚本的包做白名单。** 那仍然是在签名机上执行第三方代码，而载荷真正需要的只是一个文件权限位。

## Consequences

已记录 322 个外部包。端到端实测通过：版本门禁、清单比对、可执行位补全、原生模块探针与三关自检。

可重复性现在是**可检测**而非**有保证**：同一 commit 的两次打包是与清单比对，而不是由 lock 钉死。
