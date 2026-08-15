import { chmodSync, mkdirSync, mkdtempSync, rmSync, symlinkSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import {
  assertMacReleaseReady,
  buildEnvironment,
  bundleEnvironment,
  developerIdApplications,
  machOFiles,
  resolveNotarizationSource,
} from './desktop-mac.ts'

const IDENTITY = 'Developer ID Application: Example Owner (TEAMID1234)'
const SECOND_IDENTITY = 'Developer ID Application: Other Owner (TEAMID5678)'

function identityListing(...identities: readonly string[]): string {
  const lines = identities.map((identity, index) => `  ${String(index + 1)}) ABC "${identity}"`)
  return `${lines.join('\n')}\n     ${String(identities.length)} valid identities found\n`
}

function ready(env: NodeJS.ProcessEnv, listing = identityListing(IDENTITY)) {
  return { env, platform: 'darwin' as NodeJS.Platform, listCodeSigningIdentities: () => listing }
}

describe('developerIdApplications', () => {
  it('reads every Developer ID Application identity and ignores other certificate types', () => {
    const listing = identityListing(IDENTITY, SECOND_IDENTITY).replace(
      '     2 valid',
      '  3) DEF "Apple Development: Example Owner (TEAMID1234)"\n     3 valid',
    )
    expect(developerIdApplications(listing)).toEqual([IDENTITY, SECOND_IDENTITY])
  })
})

describe('resolveNotarizationSource', () => {
  it('prefers a Keychain profile', () => {
    expect(resolveNotarizationSource({ APPLE_KEYCHAIN_PROFILE: 'dsh-notary' })).toBe('keychain-profile')
  })

  it('accepts the complete Apple ID group', () => {
    expect(resolveNotarizationSource({
      APPLE_ID: 'owner@example.com',
      APPLE_APP_SPECIFIC_PASSWORD: 'aaaa-bbbb-cccc-dddd',
      APPLE_TEAM_ID: 'TEAMID1234',
    })).toBe('apple-id')
  })

  it('accepts the complete App Store Connect key group', () => {
    expect(resolveNotarizationSource({
      APPLE_API_KEY: '/keys/AuthKey.p8',
      APPLE_API_KEY_ID: 'KEYID',
      APPLE_API_ISSUER: 'issuer-uuid',
    })).toBe('api-key')
  })

  it('names the missing members of a partial group instead of falling through', () => {
    expect(() => resolveNotarizationSource({ APPLE_ID: 'owner@example.com' }))
      .toThrow(/missing APPLE_APP_SPECIFIC_PASSWORD, APPLE_TEAM_ID/)
  })

  it('treats a blank value as absent', () => {
    expect(() => resolveNotarizationSource({ APPLE_KEYCHAIN_PROFILE: '   ' })).toThrow(/credentials are required/)
  })

  it('requires some credential group', () => {
    expect(() => resolveNotarizationSource({})).toThrow(/credentials are required/)
  })
})

describe('assertMacReleaseReady', () => {
  const notarization = { APPLE_KEYCHAIN_PROFILE: 'dsh-notary' }

  it('returns the sole Keychain identity with its notarization mechanism', () => {
    expect(assertMacReleaseReady(ready(notarization))).toEqual({
      identity: IDENTITY,
      notarization: 'keychain-profile',
    })
  })

  it('refuses a non-macOS host', () => {
    expect(() => assertMacReleaseReady({ ...ready(notarization), platform: 'linux' }))
      .toThrow(/must be built on macOS/)
  })

  it('refuses a host with no Developer ID identity', () => {
    expect(() => assertMacReleaseReady(ready(notarization, '     0 valid identities found\n')))
      .toThrow(/No Developer ID Application identity/)
  })

  it('refuses to choose among several identities on its own', () => {
    expect(() => assertMacReleaseReady(ready(notarization, identityListing(IDENTITY, SECOND_IDENTITY))))
      .toThrow(/Set DSH_SIGN_IDENTITY to choose among 2/)
  })

  it('selects a requested identity by its team-qualified name', () => {
    const options = ready({ ...notarization, DSH_SIGN_IDENTITY: 'Other Owner (TEAMID5678)' }, identityListing(IDENTITY, SECOND_IDENTITY))
    expect(assertMacReleaseReady(options).identity).toBe(SECOND_IDENTITY)
  })

  it('refuses a request that names two certificates of the same owner', () => {
    const sameOwner = 'Developer ID Application: Example Owner (TEAMID9999)'
    const options = ready({ ...notarization, DSH_SIGN_IDENTITY: 'Example Owner' }, identityListing(IDENTITY, sameOwner))
    expect(() => assertMacReleaseReady(options)).toThrow(/matches 0 Keychain identities|does not match a valid Keychain identity/)
  })

  it('accepts the full identity string verbatim', () => {
    const options = ready({ ...notarization, DSH_SIGN_IDENTITY: SECOND_IDENTITY }, identityListing(IDENTITY, SECOND_IDENTITY))
    expect(assertMacReleaseReady(options).identity).toBe(SECOND_IDENTITY)
  })

  it('refuses a requested identity the Keychain does not hold', () => {
    expect(() => assertMacReleaseReady(ready({ ...notarization, DSH_SIGN_IDENTITY: 'Absent Owner' })))
      .toThrow(/does not match a valid Keychain identity/)
  })

  it('refuses a signable host without notarization credentials', () => {
    expect(() => assertMacReleaseReady(ready({}))).toThrow(/notarization credentials are required/)
  })
})

describe('bundleEnvironment', () => {
  it('restores only the updater signing inputs on top of the scrubbed build environment', () => {
    const bundle = bundleEnvironment({
      PATH: '/usr/bin',
      APPLE_KEYCHAIN_PROFILE: 'dsh-notary',
      DEEPSEEK_API_KEY: 'sk-live',
      TAURI_SIGNING_PRIVATE_KEY: 'untrusted comment: minisign encrypted secret key',
      TAURI_SIGNING_PRIVATE_KEY_PASSWORD: 'pw',
    })
    // The signing key reaches `tauri build`, which signs the updater artifact
    // as it produces it; every other credential stays withheld.
    expect(bundle).toEqual({
      PATH: '/usr/bin',
      TAURI_SIGNING_PRIVATE_KEY: 'untrusted comment: minisign encrypted secret key',
      TAURI_SIGNING_PRIVATE_KEY_PASSWORD: 'pw',
    })
  })

  it('adds nothing when the release runs without updater signing', () => {
    expect(bundleEnvironment({ PATH: '/usr/bin', APPLE_ID: 'owner@example.com' }))
      .toEqual({ PATH: '/usr/bin' })
  })
})

describe('buildEnvironment', () => {
  it('withholds the updater signing key from build and pack, whose npm install reaches dependency code', () => {
    expect(buildEnvironment({
      PATH: '/usr/bin',
      TAURI_SIGNING_PRIVATE_KEY: 'secret',
      TAURI_SIGNING_PRIVATE_KEY_PASSWORD: 'pw',
    })).toEqual({ PATH: '/usr/bin' })
  })

  it('withholds every release credential from build subprocesses', () => {
    const sanitized = buildEnvironment({
      PATH: '/usr/bin',
      APPLE_KEYCHAIN_PROFILE: 'dsh-notary',
      APPLE_ID: 'owner@example.com',
      APPLE_APP_SPECIFIC_PASSWORD: 'aaaa-bbbb-cccc-dddd',
      APPLE_TEAM_ID: 'TEAMID1234',
      APPLE_API_KEY: '/keys/AuthKey.p8',
      APPLE_API_KEY_ID: 'KEYID',
      APPLE_API_ISSUER: 'issuer-uuid',
      DSH_SIGN_IDENTITY: IDENTITY,
    })
    expect(sanitized).toEqual({ PATH: '/usr/bin' })
  })

  it('withholds harness credentials the build steps never need', () => {
    const sanitized = buildEnvironment({
      PATH: '/usr/bin',
      HOME: '/Users/example',
      DEEPSEEK_API_KEY: 'sk-live',
      GH_TOKEN: 'ghp-live',
      NPM_TOKEN: 'npm-live',
      AWS_SECRET_ACCESS_KEY: 'aws-live',
      SUDO_PASSWORD: 'hunter2',
    })
    expect(sanitized).toEqual({ PATH: '/usr/bin', HOME: '/Users/example' })
  })
})

describe('machOFiles', () => {
  let root: string

  beforeEach(() => {
    root = mkdtempSync(join(tmpdir(), 'dshd-macho-'))
  })

  afterEach(() => {
    rmSync(root, { recursive: true, force: true })
  })

  const write = (relative: string, bytes: Buffer): string => {
    const path = join(root, relative)
    mkdirSync(join(path, '..'), { recursive: true })
    writeFileSync(path, bytes)
    return path
  }

  const machO = (magic: number): Buffer => {
    const header = Buffer.alloc(8)
    header.writeUInt32BE(magic, 0)
    return header
  }

  it('finds Mach-O files that do not carry the executable bit', () => {
    const addon = write('Contents/Resources/app/native.node', machO(0xfeedfacf))
    chmodSync(addon, 0o644)
    expect(machOFiles(root)).toEqual([addon])
  })

  it('accepts both byte orders and universal binaries, and rejects other files', () => {
    write('Contents/Resources/readme.txt', Buffer.from('not a binary'))
    write('Contents/Resources/short', Buffer.from([0xfe]))
    const universal = write('Contents/MacOS/dshd', machO(0xcafebabe))
    const swapped = write('Contents/MacOS/helper', machO(0xcffaedfe))
    expect(new Set(machOFiles(root))).toEqual(new Set([universal, swapped]))
  })

  it('orders nested code before its container so the bundle seal is taken last', () => {
    const outer = write('Contents/MacOS/dshd', machO(0xfeedfacf))
    const inner = write('Contents/Resources/app/node_modules/pty/build/pty.node', machO(0xfeedfacf))
    expect(machOFiles(root)).toEqual([inner, outer])
  })

  it('skips symbolic links, which codesign signs through their target', () => {
    const real = write('Contents/MacOS/dshd', machO(0xfeedfacf))
    symlinkSync(real, join(root, 'Contents/MacOS/dshd-alias'))
    expect(machOFiles(root)).toEqual([real])
  })
})
