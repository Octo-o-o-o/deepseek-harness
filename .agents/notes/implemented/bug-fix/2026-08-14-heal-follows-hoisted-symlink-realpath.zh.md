# Agent Note: 通过提升符号链接的 realpath 修复 profile 回退目录

Status: implemented

[English](2026-08-14-heal-follows-hoisted-symlink-realpath.md) | 中文

## 问题

`healProfilesModuleFallback` 从每个 package.json 用 `createRequire(anchor).resolve.paths` 遍历安装目录声明的依赖闭包。`pnpm deploy` 只把应用的直接依赖提升为顶层符号链接；这些包自己的依赖住在 `.pnpm/<name>@<version>/node_modules` 里当邻居。从提升后的符号链接做 `resolve.paths` 列不出那个隔离目录，BFS 因此在 CLI 直接依赖处停下。PATH 被清空后，从已 deploy 的树启动 `dsh web` 就无法从 `$DSH_HOME/profiles/web/` 导入 `@deepseek-ai/dsh-credentials-local`（以及 `@deepseek-ai/dsh-base` 隔离闭包里的其余包）。工作区 checkout 仍能解析，因为它的提升更深；缺口只出现在桌面侧车交付的 deploy 产物上。

## 决策

`packageDirFromAnchor` 仍先探测字面量 `package.json` 路径，再在 `realpathSync` 给出不同路径时探测该 realpath。先走字面量，使既有提升或拷贝布局保持不变；realpath 这一跳与 Node ESM 跟随符号链接的行为一致，从而看见隔离 store 的邻居。[profile-plugin-bundles 决策](../architecture/2026-08-05-profile-plugin-bundles.md) 继续拥有双锚点解析和回退目录；本 note 只拥有遍历闭包时的查找原语。

## 考虑过的替代方案

**把每个隔离包都写成 `apps/cli` 的直接依赖。** 当前缺的名字会被提升到 deploy 根，但下一层嵌套依赖会以同样方式失败，而且 CLI manifest 也不再描述 CLI。

**在打包时把 `.pnpm` store 拍平进 `app/node_modules`。** 只修打包布局会把同一查找缺陷留给所有其他 `pnpm deploy` 消费者，包括以后的签名安装包。

**只用 realpath、丢掉字面量查找。** 对 ESM 正确，但会改变仍有意义的字面量查找的首选命中；先走字面量是严格增量。

## 后果

`@deepseek-ai/dsh` 的 `pnpm deploy --prod --legacy` 树在 heal 之后可以启动 `dsh web`，无需再为从 profile 目录发出的 ESM 导入额外设置 `NODE_PATH`。桌面侧车自检是第一个需要这条路径的消费者。新增的 `follows a hoisted symlink into the isolated store` 单测钉住了此前缺失的那一跳。
