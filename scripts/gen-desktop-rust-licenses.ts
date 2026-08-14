/**
 * Record the desktop application's Rust dependency licenses into
 * `apps/desktop/src-tauri/licenses.json`.
 *
 * `cargo metadata` is the only source that carries each crate's SPDX
 * expression, and it needs a Cargo toolchain and a populated registry. The
 * notices generator must run on machines that have neither, so the inventory
 * is committed and this command refreshes it. `gen-third-party-notices.ts`
 * rejects an inventory whose crate set no longer matches `Cargo.lock`, which
 * is how a dependency change is caught without running Cargo.
 *
 * The inventory covers every `Cargo.lock` entry rather than the subset that
 * links into one target: the lock is the exact set the build resolves from,
 * over-disclosure carries no obligation, and omitting a platform's crates
 * would under-disclose the artifact built for it.
 */

import { spawnSync } from 'node:child_process'
import { readFileSync, writeFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = resolve(import.meta.dirname, '..')
const MANIFEST = resolve(root, 'apps/desktop/src-tauri/Cargo.toml')
const OUT = resolve(root, 'apps/desktop/src-tauri/licenses.json')

/** One third-party crate the desktop application resolves. */
export interface CrateLicense {
  /** Crate name as published. */
  readonly name: string
  /** Exact resolved version. */
  readonly version: string
  /** SPDX expression declared by the crate, or `null` when it declares none. */
  readonly license: string | null
  /** Upstream repository, or `null` when the crate declares none. */
  readonly repository: string | null
}

interface CargoMetadataPackage {
  readonly id: string
  readonly name: string
  readonly version: string
  readonly license?: string | null
  readonly repository?: string | null
}

/**
 * Every third-party crate in one `cargo metadata` document, sorted by name and
 * version, with the workspace's own members removed.
 * @param document - parsed `cargo metadata --format-version 1` output.
 * @returns the inventory rows to commit.
 */
export function inventoryFromMetadata(document: {
  packages: readonly CargoMetadataPackage[]
  workspace_members: readonly string[]
}): CrateLicense[] {
  const members = new Set(document.workspace_members)
  return document.packages
    .filter(pkg => !members.has(pkg.id))
    .map(pkg => ({
      name: pkg.name,
      version: pkg.version,
      license: pkg.license ?? null,
      repository: pkg.repository ?? null,
    }))
    .sort((left, right) => left.name.localeCompare(right.name) || left.version.localeCompare(right.version))
}

/**
 * Every `[[package]]` entry in a `Cargo.lock`, as `name@version`.
 * @param text - the lock file's contents.
 * @returns the resolved package keys, including workspace members.
 */
export function lockPackageKeys(text: string): Set<string> {
  const keys = new Set<string>()
  for (const match of text.matchAll(/\[\[package\]\]\nname = "([^"]+)"\nversion = "([^"]+)"/g)) {
    const [, name, version] = match
    if (name !== undefined && version !== undefined) keys.add(`${name}@${version}`)
  }
  return keys
}

function main(): void {
  const result = spawnSync(
    'cargo',
    ['metadata', '--manifest-path', MANIFEST, '--format-version', '1'],
    { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 },
  )
  if (result.error !== undefined) throw result.error
  if (result.status !== 0) {
    throw new Error(`cargo metadata exited with ${String(result.status)}: ${result.stderr.trim()}`)
  }
  const document: unknown = JSON.parse(result.stdout)
  if (typeof document !== 'object' || document === null || !('packages' in document)) {
    throw new TypeError('cargo metadata did not return a package document')
  }
  const inventory = inventoryFromMetadata(document as Parameters<typeof inventoryFromMetadata>[0])
  writeFileSync(OUT, `${JSON.stringify(inventory, null, 2)}\n`)

  const undeclared = inventory.filter(crate => crate.license === null)
  console.log(`gen-desktop-rust-licenses: recorded ${String(inventory.length)} crate(s)`)
  if (undeclared.length > 0) {
    console.log(`gen-desktop-rust-licenses: ${String(undeclared.length)} crate(s) declare no license: ${undeclared.map(crate => crate.name).join(', ')}`)
  }
  const lockKeys = lockPackageKeys(readFileSync(resolve(root, 'apps/desktop/src-tauri/Cargo.lock'), 'utf8'))
  console.log(`gen-desktop-rust-licenses: Cargo.lock holds ${String(lockKeys.size)} package(s)`)
}

const invokedPath = process.argv[1]
if (invokedPath !== undefined && resolve(invokedPath) === fileURLToPath(import.meta.url)) main()
