# Agent Note：已签名的 macOS 桌面发布是一条仓库命令

Status: implemented

[English](2026-08-14-desktop-mac-signed-release.md) | 中文

## Problem

第一个经过公证的 `dshd` DMG，是由一串位于仓库之外的 shell 步骤产出的。这些步骤是真实的——深度签名、建 DMG、`notarytool submit`、`stapler staple`——但树里没有任何东西记录它们，因此已发布的产物无法从一份 checkout 重现，而 `apps/desktop/README.md` 仍然写着应用未签名、公证不在范围内。工作流文件里以注释形式重复着同一说法。

那份脚本还有三个只在重复发布时才会暴露的缺陷。它把某个人的 `Developer ID Application` 身份作为默认值，因此这条命令只在一台机器上正确。它在 `find | while` 子 shell 里签名内层二进制，而该失败分支的 `exit 1` 只结束子 shell、外层脚本继续运行，于是一个签不动的文件会产出一个几分钟后才在公证环节失败的 DMG。它还按可执行位挑选内层二进制，而这描述不了一个以 0644 模式分发的原生插件。

## Decision

`scripts/release/desktop-mac.ts` 拥有这条发布链，与仓库其他发布序列同处，并以 `pnpm run release:desktop-mac` 运行。它在一条命令内完成仓库构建、sidecar 打包、`tauri build`、sidecar 嵌入、签名、建 DMG、公证与 staple。

签名在嵌入之后。Tauri 会丢弃 sidecar 的符号链接，因此 `pack-sidecar.mjs embed` 在 bundle 存在之后才把 Node 运行时与已 deploy 的 CLI 复制进 `Contents/Resources`；在该复制之前取得的签名覆盖不到用户真正运行的载荷。

签名集合的成员资格由每个文件头部的 Mach-O magic number 决定，而不是由其权限位决定；集合按最深路径优先签名，使 bundle 封条最后取得。遍历在本进程内进行，因此一个签不动的文件会就地结束发布。不使用 `--deep`：它会用外层调用的参数重签它找到的一切，从而把 Node 运行时的 entitlements 替换成应用的。

entitlements 按可执行文件分别授予。`entitlements.node.plist` 只把 JIT、未签名可执行内存与库校验豁免授予内嵌的 Node 运行时；应用二进制与 sidecar 派生的辅助工具在 hardened runtime 下签名且不带任何 entitlements。由于授权是按文件的、而后续任何一次对 bundle 的签名都可能替换它们，发布会把 Node 运行时的 entitlements 读回来，缺失即失败——否则得到的是一个能启动、但在 V8 首次编译时死掉的应用。

bundle 在 `Contents/Resources` 内携带 `LICENSE` 与生成的 `THIRD_PARTY_NOTICES.md`，缺任一个都拒绝签名：归属义务附着在对方收到的产物上，而不是附着在他们从未克隆过的仓库上。

预检在构建之前运行，而不是在上传之前。它拒绝非 macOS 主机、没有 `Developer ID Application` 身份的 Keychain，以及候选多于一个的身份选择——后者由 `DSH_SIGN_IDENTITY` 解决，因为隐式取首个匹配会用非预期的证书签出一个发布版。公证凭据是三组完整凭据之一：Keychain profile、Apple ID 三件套，或 App Store Connect key 三件套。只给出一半的一组会指名缺失的成员，而不是回落到下一组，否则某个变量的拼写错误会表现为「未配置任何凭据」。

构建与打包步骤拿到的环境已移除全部凭据形态的变量名，而不只是 Apple 那几个——因为 sidecar 打包会跑一次执行依赖 lifecycle 脚本的 `npm install`，而它不需要任何凭据。失败的步骤只报告命令名与退出码：`notarytool` 以参数接收专用密码，回显参数的错误消息会把该密码写进终端与 CI 日志。

CI 继续在两个平台产出未签名产物。把签名路径接入 CI 需要把 Developer ID 证书与公证凭据配置为仓库 secret，那是一个独立决策；工作流注释现在陈述这一立场，而不再把证书描述为尚不存在。

## Alternatives considered

**用 `tauri build` 自带的 macOS 签名配置签名。** Tauri 签的是它自己创建的那个 bundle，而那是在 sidecar 嵌入之前。该签名覆盖不到 Node 运行时与 CLI，并且嵌入步骤会使其失效。

**保留这份 shell 脚本，原样移入仓库。** 身份默认值、被吞掉的内层签名失败、可执行位过滤，每一条都是一条静默产出错误产物的路径。发布命令不是收留「只在 Apple 公证服务处才浮现的失败」的地方。

**把 Node 的豁免授予整个 bundle。** 所有可执行文件共用一份 entitlements 更短，但那会把 JIT 与未签名可执行内存交给应用二进制以及 sidecar 派生的每个辅助工具，而它们都不在运行期编译代码。

**存在多个身份时让预检取首个 Developer ID 身份。** 这在常见情形下少一个变量，却在不常见情形下用非预期的证书静默签名。

## Consequences

持有 Developer ID 身份的维护者用一条命令即可重现已发布的 DMG，README 陈述的也是仓库真正做的事。预检的各项检查是对注入的进程输入的纯函数，因此 `scripts/release/desktop-mac.spec.ts` 无需调用 `codesign` 即可覆盖平台拒绝、身份缺失与歧义、每一组完整与不完整的凭据、凭据的隔离，以及 Mach-O 遍历的头部检测、排序与符号链接处理。

完整命令仍未被自动化证明：它需要 Developer ID 身份、公证凭据与 Apple 的服务，三者在 CI 与测试中都不存在。因此其非纯的那一半，由任何一次发布都会携带的同一份证据覆盖——挂载后的 DMG 通过 `codesign --verify --strict`、`spctl --assess` 与 `stapler validate`。
