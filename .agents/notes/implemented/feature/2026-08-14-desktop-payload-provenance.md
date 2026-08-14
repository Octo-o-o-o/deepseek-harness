# Agent Note: The desktop payload records what it installed and runs no install scripts

Status: implemented

English | [中文](2026-08-14-desktop-payload-provenance.zh.md)

## Problem

The [sidecar pack](2026-08-14-desktop-sidecar-pack.md) resolved its external dependencies at pack time with `npm install`, from ranges such as `commander ^15.0.0`. The same commit packed on two days could ship different code, and nothing compared the two. The install also ran dependency lifecycle scripts on the machine that holds the Developer ID identity, with that machine's privileges.

The Node runtime was verified against `SHASUMS256.txt` fetched from the same host at the same moment, which confirms the transfer rather than the publisher, and an interrupted download left a short file at the final cache path that failed every later run until someone deleted it by hand.

Nothing compared the three places the desktop version is declared, and the bundle recorded no version for the CLI inside it.

## Decision

`npm install --ignore-scripts`, and `restorePrebuildHelpers` then puts the executable bit back on Mach-O helper programs shipped inside `prebuilds`, which is what an install script would otherwise have done. `node-pty`'s `spawn-helper` is the one this payload needs: without it the package loads and every terminal fails with `posix_spawnp failed`.

`payload-manifest.json` records the resolved version of every external package, the CLI version, and the Node version. A pack that resolves anything else fails and names the difference; `pack-sidecar.mjs manifest` records a new resolution deliberately.

`NODE_DIGESTS` pins each archive's SHA-256 in the repository. Downloads land on a temporary name and are renamed only after they verify, and a cached archive that fails the digest is refetched instead of blocking the build.

`assertDesktopVersion` refuses a pack whose `package.json`, `tauri.conf.json`, and `Cargo.toml` versions disagree.

The pack self-check opens a pseudo-terminal and loads `sharp`, `koffi`, and the runtime's SQLite with the bundled Node. Starting the web server proves none of that, and it is exactly what a payload built for another architecture or installed without scripts gets wrong.

## Alternatives considered

**`npm ci` with a committed lockfile.** The payload's own packages are `file:` tarballs rebuilt from the checkout on every pack, and npm records their integrity in the lockfile, so the lock would be stale on each source change.

**An allowlist of packages whose scripts may run.** It still executes third-party code on the signing machine, and the only thing the payload actually needed was a file mode.

## Consequences

Recorded 322 external packages. Verified end to end: version gate, manifest match, restored helper bits, the native probe, and the three-gate self-check.

Reproducibility is now detected rather than guaranteed: two packs of one commit are compared against the manifest, not pinned by a lock.
