# Agent Note：桌面端 Rust 许可由一份纳入版本管理的清单披露

Status: implemented

[English](2026-08-14-desktop-rust-license-inventory.md) | 中文

## Problem

`THIRD_PARTY_NOTICES.md` 披露了全部 npm、vendored 与 Python 依赖，却没有披露桌面应用链接的那 483 个 crate。仓库把这些 crate 编译进一个已签名的 macOS 产物并分发，因此它们的归属与源码可获取条款适用于一次对它们只字未提的分发。

只有 `cargo metadata` 携带 crate 的 SPDX 表达式，而它需要 Cargo 工具链与已填充的 registry。notices 生成器在测试道次中运行，也运行在两者都没有的机器上，因此它不能调用 Cargo。

## Decision

`apps/desktop/src-tauri/licenses.json` 是一份纳入版本管理的清单，记录每个 crate 的名称、版本、SPDX 表达式与仓库地址。`pnpm run gen-desktop-rust-licenses` 从 `cargo metadata` 刷新它；`gen-third-party-notices.ts` 读取它并渲染 `Desktop Rust dependencies` 章节。

清单覆盖 `Cargo.lock` 的每一条，而不是某一个 target 的子集：lock 正是构建据以解析的确切集合，过度披露不产生义务，而漏掉某平台的 crate 会使为该平台构建的产物披露不足。每次生成都会把清单的 crate 集合与 lock 比对，不一致即指名差异并失败，因此依赖变更在没有 Cargo 的机器上也不会悄悄溜过。

这些 crate 走 npm 各层使用的同一条宽松许可政策，因此未经审阅的 copyleft crate 会使生成失败，而不是被吸收进一张表格。有两个标识符是该政策此前缺失而非有意排除的：`Unicode-3.0` 与 `Zlib` 本身是宽松许可，缺席只说明该集合从未见过它们。exception 默认仍不视为宽松，`LLVM-exception` 被接纳是因为它只扩大授权。

五个 MPL-2.0 crate 按名字授权。MPL-2.0 要求分发者告知接收方如何获取被覆盖的源码，因此渲染出的章节点明确切版本，并声明 crates.io 永久提供它们、且被覆盖的文件在本分发中未经修改。bundle 在 `Contents/Resources` 内携带生成的 notices，因为义务附着于产物而非仓库。

## Alternatives considered

**在 notices 生成器里直接调用 `cargo metadata`。** 这能去掉纳入版本管理的清单与过期检查，但会让一个文档门禁在每台机器和 CI 上都要求 Rust 工具链。

**只披露链接进某一个 target 的 crate。** 这份清单更小也更精确，但需要为每个交付 target 各解析一次，并让 Windows 产物的披露依赖一次 macOS 上的运行。

**把 MPL-2.0 重新归类为宽松许可。** 那会对未来每一个依赖都关闭这项检查，而不只是对这里审阅过的五个 crate；而且无论政策如何命名，MPL-2.0 的源码义务都真实存在。

## Consequences

依赖变更会使 notices 生成失败，直到清单被刷新——刷新的那台机器需要 Cargo，其它机器不需要。`scripts/gen-third-party-notices.spec.ts` 覆盖了双向的 lock 比对、版本升级、包含已授权与已接纳情形的政策分类，以及未声明许可与仓库地址时渲染出的表格行。

被交付的二进制是否真的链接了某个 crate 并未被断言；lock 按其解析结果被披露。
