# Agent Note: 载荷清单按平台分节记录

Status: implemented

[English](2026-08-15-payload-manifest-per-platform.md) | 中文

## Problem

`payload-manifest.json` 只有一张扁平的 `{cli, node, packages}` 表，而 `manifest` 步骤会用当前主机的解算结果整文件覆写。检查是双向的：载荷里有而记录里没有的包报 `+ name@version`，记录里有而未安装的报 `- name`。

可选依赖按主机解析。macOS 载荷带 `@img/sharp-darwin-arm64`，Windows 载荷带 `@img/sharp-win32-x64`。因此一张扁平表只可能对写下它的那台主机成立。

这让两个平台互相锁死。在 Windows 上构建报出 9 条差异——4 条 `win32-x64` 新增与 5 条 `darwin-arm64` 缺失，全部是同版本的变体互换，一条版本箭头都没有。接受它们会把文件翻成 win32-only，此后下一次 macOS 构建就会对称失败。任一平台都无法在不破坏另一方的前提下记录自己的解算结果，于是谁都发不出版本。

## Decision

每个平台在 `platforms['<platform>-<arch>']` 下各占一个完整分节。检查只比对当前主机的分节，仍是双向，因此漂移检测强度不减。`manifest` 步骤改为合并而非替换：它只重写自己的分节，其余分节逐字节保持原样。

当前平台分节缺失时给出的是它自己的诊断——「no section for win32-x64（已记录：darwin-arm64）；请在本平台运行 manifest 步骤」——而不是一大片增删，后者读起来像漂移，会诱导出错误的处理方式。

选择按平台切分，而不是试图区分「共有」与「平台专属」包，是为了回避一个数据本身支撑不了的分类：只记录了一个平台时，无法判断哪些条目属于共有。共有的大多数在各分节中重复出现，代价只是文件体积。

## Alternatives considered

**忽略名字看起来像平台专属的包。** 按名称模式匹配是猜测——`@img/sharp-darwin-arm64` 可以认出来，任意一个可选依赖不行——而猜错会把一个真实的包悄悄排除出比对。

**记录并集且只做单向检查**（载荷 ⊆ 清单）。这能让 Windows 构建在含 darwin 条目的清单下通过，但同时也不再能检测出「包被移除」，那是门禁的一半。

## Verification

macOS：`pack-sidecar.mjs deploy` 报告 `payload matches payload-manifest.json for darwin-arm64 (322 external package(s))`。

Windows（owner 的机器）：`manifest` 步骤记录 `win32-x64` 时为 **323 行新增、0 行删除**——darwin 分节未被触碰——随后的完整运行报告 `payload matches payload-manifest.json for win32-x64 (321 external package(s))`。

两个分节都存在后的交叉核验：darwin 分节与 Windows 运行之前逐字节相同；两个分节共有的 317 个包中，**版本不一致者为零**——平台差异被限制在带平台后缀的包上，其中没有藏着版本漂移。

其后一次 macOS 构建撞上了真实的单包漂移（`jose 6.2.8 -> 6.2.9`）；接受它恰好只改动一行，且 `win32-x64` 分节未被触碰——而这正是本 Note 所要保证的性质。

## Consequences

文件大约变成两倍大，因为共有的大多数在两个分节中都出现。这是一份能在多台主机上成立的记录所应付的代价。

每个平台需要运行一次 `manifest` 步骤来登记自己。该步骤现在从任一主机运行都是安全的——它不再可能摧毁另一平台的记录——因此此前「接受漂移前先停下报告」的要求，只适用于判断漂移**是否合理**，而不再是为了保护这个文件。
