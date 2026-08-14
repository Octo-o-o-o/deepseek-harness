# Agent Note：桌面 release profile 不再 strip host 单元

Status: implemented

[English](2026-08-14-desktop-release-profile-host-units.md) | 中文

## Problem

已签名的 macOS 发布构建不出来。`scripts/release/desktop-mac.ts` 会传 `--remap-path-prefix=$HOME=/build`，不传则 `assertNoBuildMachinePaths` 拒绝该 bundle；而带这个 flag 的构建每次都编译失败：

```
error[E0463]: can't find crate for `ctor_proc_macro`
error: .../libserde_derive-<hash>.dylib: dlopen(...): mis-aligned LINKEDIT string pool
```

`RUSTFLAGS` 除目标外还会作用到 build script 与 proc-macro crate，`[profile.release] strip = true` 于是把 rustc 在同一次构建中还要加载回来的 proc-macro 动态库 strip 掉。对一个源码路径同时被 remap 过的动态库做 strip，会留下 dyld 拒绝加载的 Mach-O，所有使用 derive 宏的 crate 因此解析失败。失败信息指向第三个 crate，看上去与这两项设置都无关。

`cargo clean` 清不掉，不带 flag 的构建又会把它藏起来——这正是发布脚本可以成功一次、之后换机器或改 flag 就必然失败的原因。

## Decision

`[profile.release.build-override] strip = false` 让 host 单元——build script 与 proc-macro crate——不再被 strip。发布的可执行文件属于 target 单元，仍然 strip，因此 bundle 不会变大。

`pack-sidecar.mjs` 新增 `bundle` 步骤持有该 remap，`apps/desktop/package.json`、桌面 CI job 与已签名发布都调用它。此前 flag 只存在于发布脚本里，因此文档里的 `pnpm --filter @deepseek-ai/dshd run build` 和 CI job 产出的 bundle 会被同一个守卫拒绝。

## Alternatives considered

**用 per-target flag（`CARGO_TARGET_<TRIPLE>_RUSTFLAGS`）加显式 `--target`。** host 与 target 相同时 cargo 仍会把 target flag 施加到 host 单元，而这里每次构建都是这种情况，proc-macro 动态库照样被 strip。

**用 cargo 编译、再用 `tauri bundle` 打包。** 它为一个并非真因的理由拆开了构建，而且会把 checkout 路径留在可执行文件里——把它去掉的正是 Tauri CLI 自己的构建环境。

**去掉 remap，只靠 `strip`。** `strip` 不会移除 registry crate 携带的 panic-location 字符串，而那正是守卫查到的东西。

## Consequences

`pnpm --filter @deepseek-ai/dshd run build` 与 `release:desktop-mac` 恢复可用。`cargo clean` 后端到端实测通过：bundle、`pack-sidecar.mjs embed` 报告载荷树无构建机路径、以及内嵌运行时的三关冒烟。
