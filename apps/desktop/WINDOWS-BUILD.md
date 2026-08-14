# Building dshd on Windows

English | [中文](WINDOWS-BUILD.zh.md)

Build the `desktop-app` branch into a Windows installer (`.exe` / `.msi`) on an x64 machine. Work through the steps in order; each one names what "it worked" looks like.

The Windows path has never run to completion. Two blockers found on CI are fixed (section D); step 4 is where the next one is most likely to appear, and section D names the one that is predicted rather than observed.

| | |
|---|---|
| Target | `x86_64-pc-windows-msvc` |
| Output | NSIS `.exe` + MSI |
| Disk | reserve 15 GB |
| Time | 40–60 minutes on a first run |

## A. Prerequisites

- **Windows 10 1809 or later / Windows 11, x64.** Windows on arm64 is untested; the pinned Node runtime is `win-x64` only.
- **Git.** `git --version` must answer.
- **Node.js 24.x.** The workspace declares `^22.19.0 || >=24.0.0` and CI uses 24. `node -v` must report `v24.x`.
- **Visual Studio 2022 Build Tools with "Desktop development with C++".** Tauri needs the MSVC linker and the Windows SDK. Build Tools alone is enough; the full IDE is not required.
- **Rust stable through rustup.** `rustup default stable` must resolve to `stable-x86_64-pc-windows-msvc`.
- **WebView2 runtime.** Bundled with Windows 11. On Windows 10, install the Evergreen runtime from Microsoft. The shell does not detect a missing runtime — the symptom is a blank window.

## B. Clone through packaging

### 1. Clone and check out the branch

```powershell
git clone https://github.com/Octo-o-o-o/deepseek-harness.git
cd deepseek-harness
git checkout desktop-app
git log --oneline -1
```

Done when `git log` shows `d3cbfc436a` or later.

### 2. Enable pnpm and install

```powershell
corepack enable
pnpm install --frozen-lockfile
```

The pnpm version is pinned by the repository's `packageManager` field; `corepack` fetches the matching one, so do not install pnpm globally. Expect around ten minutes across 239 workspaces. `native/landlock-run`'s Linux packages print `Unsupported platform` warnings — those are expected, not failures.

Done when the install exits 0.

### 3. Build the workspace

```powershell
pnpm run build
```

This emits types with `tsc`, bundles runtime with `tsdown`, then builds the Web frontend with Vite. It passes on macOS and contains no platform-specific code, but has not been run on Windows.

Done when both `apps/cli/lib/bin.js` and `apps/web/dist/index.html` exist.

### 4. Pack the sidecar payload

```powershell
node apps/desktop/scripts/pack-sidecar.mjs
```

Four things happen here: the repository is packed into release tarballs, npm installs them offline into a flat `node_modules`, `node-v24.19.0-win-x64.zip` is downloaded and checksum-verified, and a PATH-stripped boot self-check runs against the result.

**This is the step most likely to stop you.** Two Windows blockers are already fixed (section D). One more is predicted but unverified: `selfCheck()` gives the child a POSIX `PATH=/usr/bin:/bin:...` and passes no `SystemRoot`. Node on Windows may not start without `SystemRoot`, which would surface as "sidecar self-check timed out waiting for ready line". If that happens, add `SystemRoot: process.env.SystemRoot` and `TEMP: process.env.TEMP` to the `env` object inside `selfCheck` in `scripts/pack-sidecar.mjs` and run it again.

Done when the output ends with `pack-sidecar: ok` and `apps\desktop\sidecar\dist\bin\node.exe` exists.

### 5. Build the installer

```powershell
cd apps\desktop
pnpm exec tauri build
```

**Do not use `pnpm run build` here.** That script is `tauri build --bundles app && node scripts/pack-sidecar.mjs embed`, and both halves are macOS-only — `app` is the `.app` bundle target and `embed` is the macOS symlink workaround. On Windows, call `tauri build` directly so it picks the per-platform default targets.

Done when the compile succeeds and the output names the bundle paths.

## C. Where the output lands

| Kind | Path |
|---|---|
| NSIS installer | `apps\desktop\src-tauri\target\release\bundle\nsis\*.exe` |
| MSI | `apps\desktop\src-tauri\target\release\bundle\msi\*.msi` |
| Bare executable | `apps\desktop\src-tauri\target\release\dshd.exe` |

There is no Windows code-signing certificate, so SmartScreen blocks the installer once and needs "More info -> Run anyway". Say so when handing the installer to someone else.

## D. Windows blockers already fixed

**`Pack sidecar` failed with `spawn ...\node_modules\.bin\tsx ENOENT`** (fixed in `12421dc290`). `spawn` without a shell executes the file itself, and on Windows `.bin/tsx` is an extensionless POSIX script; the runnable sibling is `tsx.CMD`. `npm` follows the same rule and is `npm.cmd` there. This one was observed on CI.

**A completed build produced no installer** (fixed in `d3cbfc436a`). `tauri.conf.json` pinned `targets` to `["app", "dmg"]`, both macOS-only, so Windows emitted nothing while the workflow looked for `nsis/*.exe` and `msi/*.msi`. `targets` is now `"all"`, letting Tauri pick per-platform defaults. In the same file, `resources` mapped `dist/bin/node` into the bundle while Windows produces `node.exe`, so the source path did not exist; it is now a whole-directory mapping. The shell's `bundled_node` already probes both `bin/node` and `bin/node.exe`.

Neither fix has been verified on a real Windows host. They explain the known failures and keep macOS unchanged, but they do not prove a third and fourth blocker are absent.

## E. Reporting a failure

Three things localize almost any failure here:

```powershell
node apps\desktop\scripts\pack-sidecar.mjs 2>&1 | Tee-Object -FilePath pack.log
node -v; pnpm -v; rustc -vV
git log --oneline -1
```

A Rust compile failure in step 5 is usually an incomplete MSVC or Windows SDK install; the error names the missing `link.exe` or `.lib`.
