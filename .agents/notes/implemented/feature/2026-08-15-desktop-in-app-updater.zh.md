# Agent Note: 桌面壳的应用内更新

Status: implemented

[English](2026-08-15-desktop-in-app-updater.md) | 中文

## Problem

桌面应用此前没有更新通道。下载过某个构建的人会一直停在那一版，除非他碰巧注意到有新发布并手动重下。这份成本随用户数与发布次数增长，并且专门卡住安全修复的送达：`dshd-v0.1.7` 带了一个本地提权修复，而它自己的用户没有任何途径得知。

缺失更新通道还会反转后续工作的价值。通知、API 守卫、崩溃修复——任何改进在更新通道存在之前都无法自动触达任何人，因此即便别的条目看上去更有意思，通道也必须排在前面。

## Decision

采用 `tauri-plugin-updater` 与一对 minisign 密钥。公钥随 `tauri.conf.json` 进入安装包；插件在写盘前校验每个产物的 detached signature，因此 endpoint 即便被攻破也只能扣留或重放某个发布，无法投递自己的代码。私钥离线保管，不进仓库、不进 R2、不进任何日志。

**更新入口放在托盘，不放 Web UI。** `frontendDist` 指向的是与浏览器形态共用的 GUI，在那里做应用内更新没有意义；把控件放进去还要在共用代码里加桌面专属分支。托盘项按检查结果自改文案（`Check for Updates` / `No Updates Available` / `Update to <版本>` / `Installing Update…`），且只在点击真会产生动作的状态下可点。

**先拿到已验签的字节，再停 sidecar。** 安装器要替换内嵌 Node 运行时与已部署的 CLI；在 Windows 上，打开中的文件根本无法替换。`run_install` 先下载并验签，再 `request_stop()`，最后 `install`。下载失败时正在跑的会话还在。这次 stop 之后的 `install` 失败保不住会话：壳把主窗口带回启动页，让 Restart 可点。

**检查失败静默。** 离线或 endpoint 故障不得打断会话，因此检查失败只记录日志，托盘保留上次已知状态。`busy` 标志让第二次点击成为空操作，而不是对同一产物发起并发安装。

`latest.json` 由 `scripts/release/updater-manifest.ts` 产出。Tauri 签的是**产物**而非清单——清单只是在 URL 旁携带签名——因此发布顺序本身就是一项安全属性：缺少 `<artifact>.sig` 时脚本直接失败，这意味着清单只能在它所指向的产物已存在且已签名之后才被写出。

## 用五次失败构建换来的发布链事实

以下每一条在首次签名发布时都无法回避，且都无法从现有文档中得知：

1. **此处 Tauri 不接受 `TAURI_SIGNING_PRIVATE_KEY_PATH`。** 构建会报 `A public key has been found, but no private key`。应通过 `TAURI_SIGNING_PRIVATE_KEY` 传密钥**内容**。
2. **签名密钥无法经普通环境到达构建步骤。** `scripts/release/desktop-mac.ts` 的 `buildEnvironment` 会剥离一切凭据形态的名字（`/KEY|SECRET|TOKEN|PASSWORD/i`），两个签名变量双双命中。这道剥离是正确的——`pnpm build` 与 sidecar pack 的 `npm install` 会触及依赖代码——所以解法是 `bundleEnvironment`：只为 bundle 这一步恢复这两个名字。`bundleApp()` 只跑 `tauri build`、不安装任何东西，暴露面止于此。
3. **`notarytool store-credentials` 的 profile 会消失。** `dsh-notary` 在两次成功构建之间凭空消失了两次，且 `security` 始终无法按其文档所述的 service 名找到它。用 `security add-generic-password` 存的凭据一直稳定，因此发布改用 `APPLE_ID` 三件套，app-specific password 从自管的 Keychain 条目读取。
4. **载荷清单会在两次构建之间漂移。** 上游持续发布 patch 版本；距上次构建仅数小时的一次构建也可能因单个包而失败。这是门禁在正常工作，不是缺陷——见[按平台分节的清单 Note](../bug-fix/2026-08-15-payload-manifest-per-platform.md)。
5. **公证是最后一步，也是重做代价最高的一步。** 凭据问题出在那里会浪费整轮构建。开始发布前先用一次廉价调用（`xcrun notarytool history`）验证凭据。

## Alternatives considered

**由渲染端驱动更新 UI。** 否决：它把桌面专属行为放进共用前端，却换不来托盘已经提供的任何东西。

**静默下载、点击时安装。** 插件把下载内容以字节形式留在内存里，而载荷约 180 MB。为了在点击时省下几秒而长时间持有它是笔坏买卖；当前流程在点击时下载并安装。

**放宽 `buildEnvironment` 以携带签名密钥。** 否决：那等于把密钥交给 `npm install` 与每个依赖的 lifecycle 脚本，而这正是剥离机制要防的事。

## Verification

`cargo fmt --check`、`cargo clippy --all-targets -- -D warnings` 与 `cargo test`（72 项）通过。`scripts/release/desktop-mac.spec.ts` 覆盖了环境切分的两侧：bundle 步骤恰好恢复这两个签名名字，build/pack 步骤仍然两个都拿不到。

已完整产出一个真实签名构建并在不发布的前提下核验：公证 `Accepted`、`stapler validate` 通过、Gatekeeper 报 `source=Notarized Developer ID`、包内声明 `0.1.8` / `com.octoooo.dshd`、二进制中同时含 updater endpoint 与通知 command。updater 产物存在——`dshd.app.tar.gz`（76,261,400 字节）与 400 字节的 `.sig`——且 `updater-manifest.ts` 消费了这份真实签名。

未验证：一次真实的升级。目前尚无已发布的 `latest.json`，因此没有任何东西针对活的 endpoint 走通「检查 → 下载 → 安装 → 重启」。这是下一次发布必须首先证明的事。

## Consequences

第一个带 updater 的版本仍需手动安装——更新通道无法自我投递。此后的每一次发布都会自动到达已有用户。

密钥轮换目前没有 in-band 路径：Tauri 配置只有单个 `pubkey`、没有 keyring，因此更换密钥需要一个由旧密钥签名、内嵌新密钥的 bridge release。若在该 bridge 发布之前丢失私钥，除手动重装外没有安全的恢复途径。这是暂缓事项，不是已解决的问题。

endpoint（`/updates/latest.json`）绝不能被缓存；`site/_headers` 已为其设置 `no-store`。产物本身走内容寻址的不可变 key，可以无限期缓存。
