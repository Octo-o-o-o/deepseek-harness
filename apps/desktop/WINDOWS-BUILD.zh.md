# 在 Windows 上打包 dshd

[English](WINDOWS-BUILD.md) | 中文

在一台 x64 机器上把 `desktop-app` 分支打成 Windows 安装包（`.exe` / `.msi`）。按顺序做，每一步都写明了"成了"的判据。

Windows 这条路从未完整跑通过。CI 上撞到的两个拦路问题已修（见 D 区）；第 4 步是下一个问题最可能出现的位置，D 区也标明了哪一个是预判而非实测。

| | |
|---|---|
| 目标 | `x86_64-pc-windows-msvc` |
| 产物 | NSIS `.exe` + MSI |
| 磁盘 | 预留 15 GB |
| 耗时 | 首次 40–60 分钟 |

## A. 前置条件

- **Windows 10 1809 或更高 / Windows 11，x64。** arm64 的 Windows 未经测试，固定的 Node 运行时只有 `win-x64`。
- **Git。** `git --version` 要有输出。
- **Node.js 24.x。** workspace 声明 `^22.19.0 || >=24.0.0`，CI 用的是 24。`node -v` 应显示 `v24.x`。
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

`git log` 显示 `d3cbfc436a` 或更新即为完成。

### 2. 启用 pnpm 并安装依赖

```powershell
corepack enable
pnpm install --frozen-lockfile
```

pnpm 版本由仓库的 `packageManager` 字段钉死，`corepack` 会取对应版本，因此不要全局安装 pnpm。239 个 workspace，预计十分钟上下。`native/landlock-run` 的 Linux 包会打印 `Unsupported platform` 警告，那是预期内的，不是失败。

安装退出码为 0 即为完成。

### 3. 构建 workspace

```powershell
pnpm run build
```

先用 `tsc` 出类型，再用 `tsdown` 打运行时，最后用 Vite 构建 Web 前端。这一步在 macOS 上是通的，且不含平台相关代码，但没有在 Windows 上跑过。

`apps/cli/lib/bin.js` 与 `apps/web/dist/index.html` 都存在即为完成。

### 4. 打 sidecar 载荷

```powershell
node apps/desktop/scripts/pack-sidecar.mjs
```

这一步做四件事：把仓库打成 release tarball，用 npm 离线装成扁平的 `node_modules`，下载并校验 `node-v24.19.0-win-x64.zip`，最后剥掉 PATH 对结果跑一次启动自检。

**这是最可能卡住你的一步。** 两个 Windows 拦路问题已修（见 D 区）。还有一个是预判而非实测：`selfCheck()` 给子进程写的是 POSIX 的 `PATH=/usr/bin:/bin:...`，且没有传 `SystemRoot`。Windows 上 Node 缺 `SystemRoot` 可能起不来，表现为"sidecar self-check timed out waiting for ready line"。真撞上了，在 `scripts/pack-sidecar.mjs` 的 `selfCheck` 里给 `env` 补上 `SystemRoot: process.env.SystemRoot` 与 `TEMP: process.env.TEMP`，再跑一次。

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

两处修复都**没有在真实 Windows 主机上验证过**。它们解释了已知的失败，也保持 macOS 不变，但不能证明不存在第三、第四个拦路问题。

## E. 失败了怎么报

三样东西基本能定位这里的任何失败：

```powershell
node apps\desktop\scripts\pack-sidecar.mjs 2>&1 | Tee-Object -FilePath pack.log
node -v; pnpm -v; rustc -vV
git log --oneline -1
```

第 5 步的 Rust 编译失败通常是 MSVC 或 Windows SDK 没装全，报错里会点名缺哪个 `link.exe` 或哪个 `.lib`。
