# Agent Note: Heal the profile fallback through a hoisted symlink's realpath

Status: implemented

English | [中文](2026-08-14-heal-follows-hoisted-symlink-realpath.zh.md)

## Problem

`healProfilesModuleFallback` walks the installation's declared dependency closure by probing `createRequire(anchor).resolve.paths` from each package.json. `pnpm deploy` hoists only direct app dependencies as top-level symlinks; those packages' own dependencies live as siblings inside `.pnpm/<name>@<version>/node_modules`. `resolve.paths` from the hoisted symlink does not list that isolated directory, so BFS stopped after the CLI's direct dependencies. A PATH-stripped `dsh web` from a deployed tree then failed to import `@deepseek-ai/dsh-credentials-local` (and the rest of `@deepseek-ai/dsh-base`'s isolated closure) from `$DSH_HOME/profiles/web/`. A workspace checkout still resolved those packages because its hoist is deeper; the gap appeared only on the deployed artifact the desktop sidecar ships.

## Decision

`packageDirFromAnchor` still probes the lexical `package.json` path first, then — when `realpathSync` yields a different path — probes that realpath as well. Lexical-first keeps every existing hoisted or copied layout unchanged. The realpath hop matches Node ESM's symlink-following and exposes the isolated-store neighbors. The [profile-plugin-bundles decision](../architecture/2026-08-05-profile-plugin-bundles.md) still owns two-anchor resolution and the fallback directory; this note owns only the lookup primitive used while walking the closure.

## Alternatives considered

**Declare every isolated package as a direct `apps/cli` dependency.** That would hoist the current missing names into the deploy root, but the next nested dependency would fail the same way, and the CLI manifest would stop describing the CLI.

**Flatten the `.pnpm` store into `app/node_modules` at pack time.** A pack-only layout would hide the same lookup bug for every other `pnpm deploy` consumer, including a future signed installer.

**Replace lexical lookup with realpath-only.** Correct for ESM, but it would change the first hit in layouts that still have a meaningful lexical walk; lexical-first is strictly additive.

## Consequences

A `pnpm deploy --prod --legacy` tree of `@deepseek-ai/dsh` can boot `dsh web` after heal without extra `NODE_PATH` for ESM imports from a profile directory. The desktop sidecar self-check is the first consumer that requires this. The new `follows a hoisted symlink into the isolated store` unit test pins the previously missing hop.
