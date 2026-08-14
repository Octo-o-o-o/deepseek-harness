# Building dshd on Windows

English | [中文](WINDOWS-BUILD.zh.md)

Build the `desktop-app` branch into a Windows installer (`.exe` / `.msi`) on an x64 machine. Work through the steps in order; each one names what "it worked" looks like.

The Windows path ran to completion on 2026-08-14 on a real Windows 11 x64 host (Node 22.22.0, rustc 1.94.0 msvc, producing both NSIS and MSI). The two blockers fixed earlier on CI and the four fixed during that verified run are recorded in section D.

| | |
|---|---|
| Target | `x86_64-pc-windows-msvc` |
| Output | NSIS `.exe` + MSI |
| Disk | reserve 15 GB |
| Time | 40–60 minutes on a first run |

## A. Prerequisites

- **Windows 10 1809 or later / Windows 11, x64.** Windows on arm64 is untested; the pinned Node runtime is `win-x64` only.
- **Git.** `git --version` must answer.
- **Node.js 24.x.** The workspace declares `^22.19.0 || >=24.0.0` and CI uses 24. `node -v` must report `v24.x`. 22.22.0 (inside the engines range) has been verified on a real host.
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

Done when `git log` shows `cbb9155e07` or later.

### 2. Enable pnpm and install

```powershell
corepack enable
pnpm install --frozen-lockfile
```

The pnpm version is pinned by the repository's `packageManager` field; `corepack` fetches the matching one, so do not install pnpm globally. If `corepack enable` fails with EPERM (it cannot write shims into the Node install directory without admin rights), invoke `corepack pnpm ...` instead — the version still comes from `packageManager`. Expect around ten minutes across 239 workspaces. `native/landlock-run`'s Linux packages print `Unsupported platform` warnings — those are expected, not failures.

Done when the install exits 0.

### 3. Build the workspace

```powershell
pnpm run build
```

This emits types with `tsc`, bundles runtime with `tsdown`, then builds the Web frontend with Vite. It contains no platform-specific code and passes on both hosts.

Done when both `apps/cli/lib/bin.js` and `apps/web/dist/index.html` exist.

### 4. Pack the sidecar payload

```powershell
node apps/desktop/scripts/pack-sidecar.mjs
```

Four things happen here: the repository is packed into release tarballs, npm installs them offline into a flat `node_modules`, `node-v24.19.0-win-x64.zip` is downloaded and checksum-verified, and a PATH-stripped boot self-check runs against the result.

**This is the step that produced every Windows blocker in section D**, all of them now fixed. One behavior is not a bug and has no fix: a first boot of a freshly packed payload can exceed the 15 s self-check timeout while Defender scans the new files. Once they are warm, `node apps/desktop/scripts/pack-sidecar.mjs check` passes on its own — the `deploy` and `runtime` steps do not need repeating.

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

The following four were hit and fixed on a real Windows 11 x64 host on 2026-08-14:

**`pack-sidecar` failed with `spawn EINVAL` (errno -4071).** Node ≥18.20, hardened for CVE-2024-27980, refuses to spawn `.cmd`/`.bat` without `shell: true`. `12421dc290`'s switch to `tsx.CMD` fixed ENOENT but hit EINVAL on current Node. Fix: `run()`/`capture()` in `pack-sidecar.mjs` pass `shell: true` for `.cmd`/`.bat` commands on win32.

**`pack.ts` failed with `spawnSync pnpm ENOENT`.** An extensionless `pnpm` resolves as `pnpm.exe` on Windows, which does not exist (PATH holds only shims). Fix: `scripts/release/process.ts` routes non-`.exe`/`.com` commands through a shell on win32.

**tar failed with `Cannot connect to C: resolve failed`.** Git's GNU tar resolves before System32 bsdtar on PATH; GNU tar reads the `C:` drive prefix as a remote host and cannot extract zip (the Node runtime archive is a zip). Fix: `tarball.ts` and `pack-sidecar.mjs` use `%SystemRoot%\System32\tar.exe` explicitly on win32.

**Self-check timeout (as predicted).** `selfCheck`'s env now carries `SystemRoot` and `TEMP`. That was necessary but not sufficient: the first run still timed out, and reproducing the same spawn by hand started the sidecar immediately, which identified the remaining delay as Defender scanning a freshly written payload rather than a missing variable. See the cold-cache note in B4.

Every one of these fixes is guarded by `process.platform === 'win32'`, and macOS was re-verified against them: `pack-sidecar.mjs check` passes, `scripts/release` unit tests pass 18/18, and both `tarball.ts` tar call sites read a real tarball. The Windows side was verified by a complete build that produced both NSIS and MSI.

Two **runtime** (first launch after install) blockers were then hit and fixed on the same host:

**First launch failed with "sidecar stdout closed before the ready line"; sidecar.log held `Assertion failed: ncrypto::CSPRNG(nullptr, 0)`.** The shell's `env.rs` whitelist omitted `SystemRoot`: after `env_clear()`, Node's crypto init fails and the process aborts before any JS runs. libuv-based parents silently re-inject `SystemRoot`, so only a Rust `Command` child exposes it. Fix: `SystemRoot` and `SystemDrive` joined `INHERITED_ENV`.

**The sidecar then died with `EBUSY: watch ...\desktop.lock`.** The shell held the lock file with `share_mode(0)` (exclusive), while the sidecar's hot-reload watch of `<DSH_HOME>/cordis.patch.yml` gives chokidar the whole `.dsh` directory; the per-file `fs.watch` hits the exclusive handle and crashes Node. Fix: `lock.rs` now opens fully shared and holds a non-blocking exclusive `LockFileEx` byte-range lock — a second instance is still refused, innocent readers are not.

## E. Reporting a failure

Three things localize almost any failure here:

```powershell
node apps\desktop\scripts\pack-sidecar.mjs 2>&1 | Tee-Object -FilePath pack.log
node -v; pnpm -v; rustc -vV
git log --oneline -1
```

A Rust compile failure in step 5 is usually an incomplete MSVC or Windows SDK install; the error names the missing `link.exe` or `.lib`.
