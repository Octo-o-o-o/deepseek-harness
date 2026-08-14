# Agent Note: The desktop release profile leaves host units unstripped

Status: implemented

English | [中文](2026-08-14-desktop-release-profile-host-units.zh.md)

## Problem

The signed macOS release could not build. `scripts/release/desktop-mac.ts` passes `--remap-path-prefix=$HOME=/build`, without which `assertNoBuildMachinePaths` refuses the bundle, and every build carrying that flag failed to compile:

```
error[E0463]: can't find crate for `ctor_proc_macro`
error: .../libserde_derive-<hash>.dylib: dlopen(...): mis-aligned LINKEDIT string pool
```

`RUSTFLAGS` reach build scripts and proc-macro crates as well as the target, and `[profile.release] strip = true` then strips the proc-macro dynamic library that rustc has to load back during the same build. Stripping one whose source paths were also remapped leaves a Mach-O that dyld refuses, so every crate using a derive macro fails to resolve it. The failure looks unrelated to either setting because it names a third crate.

`cargo clean` does not clear it and a plain unflagged build hides it, which is why the release script could work once and then fail on every later machine or after any flag change.

## Decision

`[profile.release.build-override] strip = false` keeps host units — build scripts and proc-macro crates — unstripped. The shipped executable is a target unit and stays stripped, so the bundle does not grow.

`pack-sidecar.mjs` gains a `bundle` step that owns the remap, and `apps/desktop/package.json`, the desktop CI job, and the signed release all call it. The flag lived only in the release script before, so the documented `pnpm --filter @deepseek-ai/dshd run build` and the CI job produced bundles that the same guard rejected.

## Alternatives considered

**Per-target flags (`CARGO_TARGET_<TRIPLE>_RUSTFLAGS`) with an explicit `--target`.** Cargo applies target flags to host units as well when host and target match, which is every build here, so the proc-macro dylibs are stripped exactly as before.

**Compile with cargo and bundle with `tauri bundle`.** It splits the build for a reason that is not the actual cause, and it leaves the checkout path in the executable because the Tauri CLI's own build environment is what removes it.

**Drop the remap and rely on `strip`.** `strip` does not remove the panic-location strings the registry crates carry, which is what the guard finds.

## Consequences

`pnpm --filter @deepseek-ai/dshd run build` and `release:desktop-mac` build again. Verified end to end after `cargo clean`: bundle, `pack-sidecar.mjs embed` reporting no build-machine paths, and the three-gate smoke of the bundled runtime.
