# Agent Note: Desktop sidecar pack is a pnpm deploy plus a pinned Node runtime

Status: implemented

English | [中文](2026-08-14-desktop-sidecar-pack.zh.md)

## Problem

The desktop shell can spawn a checkout `dsh web` during development, but a shipped `.app` cannot assume `node` or `pnpm` on PATH. `pnpm deploy --prod` of `@deepseek-ai/dsh` is the installation shape the CLI already owns; that tree is not a complete Node-visible closure on its own. Direct app dependencies are hoisted as symlinks, Service Definition packages live only as peers, and ESM imports from a realpathed package do not see `$DSH_HOME/profiles/node_modules`.

## Decision

`apps/desktop/scripts/pack-sidecar.mjs` builds `sidecar/dist` from three steps: `pnpm --filter @deepseek-ai/dsh deploy --prod --legacy`, a sha256-checked Node v24.19.0 runtime for the host triple, and a PATH-stripped boot that must print the ready line, answer `GET /` with `__DSH_BOOT__`, and exit after SIGTERM. The script then hoists every `.pnpm` package into `app/node_modules` so parent-walk matches what a workspace checkout already sees. `apps/cli` lists the Service Definition packages that `pnpm deploy --prod` otherwise omits (`dsh-timeout`, `dsh-invariants`, `dsh-subprocess`, and the rest of that peer set) because the CLI is the assembly surface. Tauri `bundle.resources` maps `sidecar/dist/bin/node` → `bin/node` and `sidecar/dist/app` → `app`, but the copy drops directory symlinks, so `pack-sidecar.mjs embed` re-copies the self-checked tree with `cp -a` after `tauri build`. The shell locates `bin/node` and `app/lib/bin.js` under `.app/Contents/Resources`. The [heal realpath lookup](../bug-fix/2026-08-14-heal-follows-hoisted-symlink-realpath.md) is required so boot can still flatten the isolated store into `$DSH_HOME/profiles/node_modules`.

## Alternatives considered

**Ship PATH `node` and a checkout `apps/cli/lib/bin.js` only.** That is the M0 dev path and cannot pass a clean-machine self-check.

**Flatten by copying the `.pnpm` store into real directories.** Correct but doubles the payload; hoisted symlinks keep one copy and match Node's symlink-following.

**Put the missing Service Definitions on `@deepseek-ai/dsh-base` instead of the CLI.** The bundle already lists row plugins; the prompt's assembly surface for a deployable `dsh` is `apps/cli`.

## Consequences

A PATH-stripped `sidecar/dist/bin/node sidecar/dist/app/lib/bin.js web --port 0 --host 127.0.0.1` is the pack gate. The `.app` is unsigned; notarize stays out of this change. Windows unpack of the Node zip is deferred to the desktop CI lane.
