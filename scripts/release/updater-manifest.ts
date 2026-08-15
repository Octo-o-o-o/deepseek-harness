/**
 * Build the updater manifest (`latest.json`) from signed release artifacts.
 *
 * The plugin signs each artifact, not this file: `latest.json` only carries the
 * detached signature next to the URL that serves the bytes it signs. A tampered
 * manifest can therefore withhold or replay a release, but cannot introduce
 * code — which is why the publish order matters and is enforced here by
 * construction: this script refuses to emit a manifest for an artifact whose
 * signature file is missing, so the manifest can only be written after the
 * artifacts it points at exist and are signed.
 *
 * Usage:
 *   tsx scripts/release/updater-manifest.ts --version 0.1.8 --base <url-prefix> \
 *     --darwin-aarch64 <path-to.app.tar.gz> [--windows-x86_64 <path-to-setup.exe>] \
 *     --out <latest.json>
 *
 * Each artifact path must have a sibling `<path>.sig` produced by
 * `tauri build` with `TAURI_SIGNING_PRIVATE_KEY` set.
 */

import { readFile, writeFile } from 'node:fs/promises'
import { basename } from 'node:path'

/** Platform keys the updater plugin resolves against `{{target}}-{{arch}}`. */
const PLATFORM_FLAGS = {
  '--darwin-aarch64': 'darwin-aarch64',
  '--windows-x86_64': 'windows-x86_64',
} as const

/** One platform entry in the manifest. */
interface PlatformEntry {
  /** Detached minisign signature of the artifact at `url`. */
  signature: string
  /** Absolute URL the updater downloads. */
  url: string
}

/** The manifest the updater endpoint serves. */
interface UpdaterManifest {
  version: string
  notes: string
  pub_date: string
  platforms: Record<string, PlatformEntry>
}

/**
 * Read the sibling `.sig` file for an artifact.
 *
 * @param artifact - path to the signed artifact.
 * @returns the signature text with surrounding whitespace removed.
 */
async function readSignature(artifact: string): Promise<string> {
  const path = `${artifact}.sig`
  try {
    return (await readFile(path, 'utf8')).trim()
  } catch {
    throw new Error(
      `updater-manifest: missing signature ${path}. Build with TAURI_SIGNING_PRIVATE_KEY set so tauri emits it.`,
    )
  }
}

/**
 * Parse `--flag value` pairs.
 *
 * @param argv - process arguments after the script name.
 * @returns the flag map.
 */
function parseArgs(argv: readonly string[]): Map<string, string> {
  const args = new Map<string, string>()
  for (let i = 0; i < argv.length; i += 2) {
    const flag = argv[i]
    const value = argv[i + 1]
    if (flag === undefined || !flag.startsWith('--') || value === undefined) {
      throw new Error(`updater-manifest: expected --flag value pairs, got ${JSON.stringify(argv.slice(i))}`)
    }
    args.set(flag, value)
  }
  return args
}

/**
 * Assemble and write the manifest.
 *
 * @param argv - process arguments after the script name.
 * @returns the manifest that was written.
 */
export async function buildManifest(argv: readonly string[]): Promise<UpdaterManifest> {
  const args = parseArgs(argv)
  const version = args.get('--version')
  const base = args.get('--base')
  const out = args.get('--out')
  if (version === undefined || base === undefined || out === undefined) {
    throw new Error('updater-manifest: --version, --base, and --out are required')
  }
  const platforms: Record<string, PlatformEntry> = {}
  for (const [flag, key] of Object.entries(PLATFORM_FLAGS)) {
    const artifact = args.get(flag)
    if (artifact === undefined) continue
    platforms[key] = {
      signature: await readSignature(artifact),
      url: `${base.replace(/\/$/, '')}/${basename(artifact)}`,
    }
  }
  if (Object.keys(platforms).length === 0) {
    throw new Error(`updater-manifest: no artifacts given (${Object.keys(PLATFORM_FLAGS).join(', ')})`)
  }
  const manifest: UpdaterManifest = {
    version,
    notes: args.get('--notes') ?? `dshd ${version}`,
    // Stamped at build time; the updater only compares versions, so this is
    // informational for anyone reading the endpoint.
    pub_date: new Date().toISOString(),
    platforms,
  }
  await writeFile(out, JSON.stringify(manifest, null, 2) + '\n')
  return manifest
}

if (import.meta.url === `file://${process.argv[1]}`) {
  buildManifest(process.argv.slice(2))
    .then((manifest) => {
      console.log(
        `updater-manifest: wrote ${manifest.version} for ${Object.keys(manifest.platforms).join(', ')}`,
      )
    })
    .catch((error: unknown) => {
      console.error(error instanceof Error ? error.message : String(error))
      process.exitCode = 1
    })
}
