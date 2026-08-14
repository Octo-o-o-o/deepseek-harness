# 在 Windows 上打包 dshd

[English](WINDOWS-BUILD.md) | 中文

在一台 x64 机器上把 `desktop-app` 分支打成 Windows 安装包（`.exe` / `.msi`）。按顺序做，每一步都写明了"成了"的判据。

Windows 这条路已于 2026-08-14 在真实 Windows 11 x64 主机上完整跑通（Node 22.22.0、rustc 1.94.0 msvc，产出 NSIS + MSI）。CI 上修掉的两个问题与本次实测修掉的四个问题都记录在 D 区。

| | |
|---|---|
| 目标 | `x86_64-pc-windows-msvc` |
| 产物 | NSIS `.exe` + MSI |
| 磁盘 | 预留 15 GB |
| 耗时 | 首次 40–60 分钟 |

## A. 前置条件

- **Windows 10 1809 或更高 / Windows 11，x64。** arm64 的 Windows 未经测试，固定的 Node 运行时只有 `win-x64`。
- **Git。** `git --version` 要有输出。
- **Node.js 24.x。** workspace 声明 `^22.19.0 || >=24.0.0`，CI 用的是 24。`node -v` 应显示 `v24.x`。22.22.0（在 engines 范围内）已在真实主机上验证可行。
- **Visual Studio 2022 生成工具，勾选"使用 C++ 的桌面开发"。** Tauri 需要 MSVC 链接器与 Windows SDK。只装生成工具即可，不必装完整 IDE。
- **经 rustup 安装的 Rust stable。** `rustup default stable` 应解析为 `stable-x86_64-pc-windows-msvc`。
- **WebView2 运行时。** Windows 11 自带。Windows 10 需从微软官网安装 Evergreen 版。壳不做缺失检测，缺了的表现是空白窗口。

## B. 从拉代码到出包

### 1. 克隆并切到分支

```powershell
git clone https://github.com/Octo-o-o-o/deepseek-harness.git
cd deepseek-harness
git checkout desktop-app
git log --oneline -1
```

`git log` 显示 `cbb9155e07` 或更新即为完成。

### 2. 启用 pnpm 并安装依赖

```powershell
corepack enable
pnpm install --frozen-lockfile
```

pnpm 版本由仓库的 `packageManager` 字段钉死，`corepack` 会取对应版本，因此不要全局安装 pnpm。若 `corepack enable` 因写不了 Node 安装目录报 EPERM（无管理员权限），改用 `corepack pnpm ...` 调用即可，版本同样由 `packageManager` 决定。239 个 workspace，预计十分钟上下。`native/landlock-run` 的 Linux 包会打印 `Unsupported platform` 警告，那是预期内的，不是失败。

安装退出码为 0 即为完成。

### 3. 构建 workspace

```powershell
pnpm run build
```

先用 `tsc` 出类型，再用 `tsdown` 打运行时，最后用 Vite 构建 Web 前端。这一步不含平台相关代码，两个平台都通过。

`apps/cli/lib/bin.js` 与 `apps/web/dist/index.html` 都存在即为完成。

### 4. 打 sidecar 载荷

```powershell
node apps/desktop/scripts/pack-sidecar.mjs
```

这一步做四件事：把仓库打成 release tarball，用 npm 离线装成扁平的 `node_modules`，下载并校验 `node-v24.19.0-win-x64.zip`，最后剥掉 PATH 对结果跑一次启动自检。

**D 区的每一个 Windows 拦路问题都出自这一步**，现已全部修掉。有一个现象不是缺陷、也没有修法：全新 payload 首跑可能被 Defender 实时扫描拖过 15 秒自检超时。文件热了之后单独重跑 `node apps/desktop/scripts/pack-sidecar.mjs check` 即可通过，`deploy` 与 `runtime` 两步无需重来。

输出以 `pack-sidecar: ok` 结束、且 `apps\desktop\sidecar\dist\bin\node.exe` 存在，即为完成。

### 5. 打安装包

```powershell
cd apps\desktop
pnpm exec tauri build
```

**这里不要用 `pnpm run build`。** 那个脚本是 `tauri build --bundles app && node scripts/pack-sidecar.mjs embed`，两截都是 macOS 专用——`app` 是 `.app` 包目标，`embed` 是 macOS 的符号链接补救。Windows 上直接调 `tauri build`，让它按平台取默认目标集。

编译通过、输出里给出 bundle 路径，即为完成。

## C. 产物在哪

| 类型 | 路径 |
|---|---|
| NSIS 安装器 | `apps\desktop\src-tauri\target\release\bundle\nsis\*.exe` |
| MSI | `apps\desktop\src-tauri\target\release\bundle\msi\*.msi` |
| 裸可执行文件 | `apps\desktop\src-tauri\target\release\dshd.exe` |

Windows 侧没有代码签名证书，安装时 SmartScreen 会拦一次，需要"更多信息 → 仍要运行"。把安装包交给别人时请说明这一点。

## D. 已修掉的 Windows 拦路问题

**`Pack sidecar` 报 `spawn ...\node_modules\.bin\tsx ENOENT`**（修于 `12421dc290`）。不带 shell 的 `spawn` 执行的是文件本身，而 Windows 上 `.bin/tsx` 是无扩展名的 POSIX 脚本，可执行的同胞是 `tsx.CMD`。`npm` 同理，在那里是 `npm.cmd`。这一条是 CI 上实测到的。

**跑完也拿不到安装包**（修于 `d3cbfc436a`）。`tauri.conf.json` 把 `targets` 钉死为 `["app", "dmg"]`，两个都是 macOS 专有，Windows 因此不产出任何东西，而 workflow 去找 `nsis/*.exe` 与 `msi/*.msi`。`targets` 现已改为 `"all"`，由 Tauri 按平台取默认集。同一文件里，`resources` 把 `dist/bin/node` 映射进包，而 Windows 产出的是 `node.exe`，该源路径并不存在；现已改为整目录映射。壳侧的 `bundled_node` 本就同时探测 `bin/node` 与 `bin/node.exe`。

以下四处于 2026-08-14 在真实 Windows 11 x64 主机上实测撞到并修复：

**`pack-sidecar` 报 `spawn EINVAL`（errno -4071）。** Node ≥18.20 出于 CVE-2024-27980 的加固，拒绝在不带 `shell: true` 的情况下 spawn `.cmd`/`.bat`。`12421dc290` 把 `tsx` 换成 `tsx.CMD` 只解决了 ENOENT，在新 Node 上撞 EINVAL。修复：`pack-sidecar.mjs` 的 `run()`/`capture()` 对 win32 的 `.cmd`/`.bat` 命令加 `shell: true`。

**`pack.ts` 报 `spawnSync pnpm ENOENT`。** 无扩展名的 `pnpm` 在 Windows 上按 `pnpm.exe` 解析（PATH 里只有 shim，没有 `.exe`）。修复：`scripts/release/process.ts` 在 win32 上对非 `.exe`/`.com` 命令走 shell。

**tar 报 `Cannot connect to C: resolve failed`。** PATH 上 Git 的 GNU tar 先于 System32 的 bsdtar 被解析；GNU tar 把 `C:` 盘符前缀当远程主机，也不能解 zip（node 运行时正是 zip）。修复：`tarball.ts` 与 `pack-sidecar.mjs` 在 win32 显式使用 `%SystemRoot%\System32\tar.exe`。

**self-check 超时（与预判一致）。** `selfCheck` 的 env 现已带上 `SystemRoot` 与 `TEMP`。这一条必要但不充分：首跑仍然超时，而手动用同样参数复现 spawn 时 sidecar 秒起，由此定位到剩余延迟是 Defender 扫描新落盘的 payload，而非缺变量。见 B4 的冷缓存说明。

以上每处修复都由 `process.platform === 'win32'` 守卫，且 macOS 侧已对着它们复验：`pack-sidecar.mjs check` 通过、`scripts/release` 单测 18/18 通过、`tarball.ts` 两个 tar 调用点都读通了真实 tarball。Windows 侧则由一次产出 NSIS + MSI 的完整构建验证。

另有两处**运行时**（安装后首启）拦路问题随后在同一真机实测修复：

**首启报 "sidecar stdout closed before the ready line"，sidecar.log 为 `Assertion failed: ncrypto::CSPRNG(nullptr, 0)`。** 壳的 `env.rs` 白名单漏了 `SystemRoot`：`env_clear()` 后 Node 的加密初始化失败，进程在跑任何 JS 之前就 abort。libuv 系父进程会隐式补 `SystemRoot`，因此只有 Rust `Command` 拉起的子进程暴露此问题。修复：`INHERITED_ENV` 加入 `SystemRoot` 与 `SystemDrive`。

**随后 sidecar 死于 `EBUSY: watch ...\desktop.lock`。** 壳用 `share_mode(0)` 完全排他持有锁文件，而 sidecar 对 `<DSH_HOME>/cordis.patch.yml` 的热重载 watch 会把 `.dsh` 整个目录交给 chokidar 扫描，per-file `fs.watch` 撞上排他句柄即崩。修复：`lock.rs` 改为全共享打开 + `LockFileEx` 非阻塞排他字节锁——第二个实例照样被拒（单实例语义不变），读取者不再被伤及。

## E. 失败了怎么报

三样东西基本能定位这里的任何失败：

```powershell
node apps\desktop\scripts\pack-sidecar.mjs 2>&1 | Tee-Object -FilePath pack.log
node -v; pnpm -v; rustc -vV
git log --oneline -1
```

第 5 步的 Rust 编译失败通常是 MSVC 或 Windows SDK 没装全，报错里会点名缺哪个 `link.exe` 或哪个 `.lib`。
