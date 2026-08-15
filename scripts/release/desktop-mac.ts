/**
 * Signed and notarized macOS release for the `dshd` desktop application.
 *
 * The application bundle is signed after `pack-sidecar.mjs embed` copies the
 * Node runtime and the deployed CLI into `Contents/Resources`, because a
 * signature taken before that copy does not cover the embedded payload. Every
 * Mach-O file in the finished bundle is therefore signed here, innermost path
 * first, before the bundle seal is taken.
 */

import { spawnSync } from 'node:child_process'
import { openSync, readSync, closeSync, existsSync, readdirSync, mkdtempSync, rmSync, cpSync, symlinkSync, readFileSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { basename, dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const DEVELOPER_ID_PREFIX = 'Developer ID Application:'
const VOLUME_NAME = 'dshd'
/** Bundle-relative location of the embedded Node runtime, per `tauri.conf.json` resources. */
const NODE_RUNTIME_SUFFIX = '/Contents/Resources/bin/node'
/** Entitlement the embedded Node runtime must still carry after the bundle seal. */
const NODE_REQUIRED_ENTITLEMENT = 'com.apple.security.cs.allow-jit'
/** Mach-O and universal-binary magic numbers, in both byte orders. */
const MACH_O_MAGIC = new Set([0xfeedface, 0xfeedfacf, 0xcefaedfe, 0xcffaedfe, 0xcafebabe, 0xbebafeca])

/** Credential mechanism `notarytool` will use. */
export type NotarizationSource = 'keychain-profile' | 'apple-id' | 'api-key'

/** Process inputs the preflight reads, injected so the checks stay testable. */
export interface MacReleaseOptions {
  /** Environment the release runs under. */
  readonly env: NodeJS.ProcessEnv
  /** Platform that will run `codesign`. */
  readonly platform: NodeJS.Platform
  /** Valid code-signing identities visible to the build user. */
  readonly listCodeSigningIdentities: () => string
}

/** Non-secret release selections the preflight confirmed. */
export interface MacReleasePlan {
  /** Full `Developer ID Application: …` identity passed to `codesign`. */
  readonly identity: string
  /** Credential mechanism `notarytool` will use. */
  readonly notarization: NotarizationSource
}

function environmentValue(env: NodeJS.ProcessEnv, name: string): string | undefined {
  const value = env[name]?.trim()
  return value === undefined || value === '' ? undefined : value
}

/**
 * Every `Developer ID Application` identity in `security find-identity` output.
 * @param output - verbatim `security find-identity -v -p codesigning` stdout.
 * @returns the quoted identity strings, in the order the tool listed them.
 */
export function developerIdApplications(output: string): readonly string[] {
  const identities: string[] = []
  for (const match of output.matchAll(/"(Developer ID Application:[^"]+)"/g)) {
    const [, identity] = match
    if (identity !== undefined) identities.push(identity)
  }
  return identities
}

function resolveCredentialGroup(
  env: NodeJS.ProcessEnv,
  names: readonly string[],
  source: NotarizationSource,
): NotarizationSource | undefined {
  const present = names.filter(name => environmentValue(env, name) !== undefined)
  if (present.length === 0) return undefined
  if (present.length !== names.length) {
    const missing = names.filter(name => environmentValue(env, name) === undefined)
    throw new Error(`Incomplete macOS notarization credentials: missing ${missing.join(', ')}`)
  }
  return source
}

/**
 * Select the notarization credential group, requiring each group to be complete.
 * @param env - environment the release runs under.
 * @returns the credential mechanism `notarytool` will use.
 */
export function resolveNotarizationSource(env: NodeJS.ProcessEnv): NotarizationSource {
  if (environmentValue(env, 'APPLE_KEYCHAIN_PROFILE') !== undefined) return 'keychain-profile'
  const appleId = resolveCredentialGroup(env, ['APPLE_ID', 'APPLE_APP_SPECIFIC_PASSWORD', 'APPLE_TEAM_ID'], 'apple-id')
  if (appleId !== undefined) return appleId
  const apiKey = resolveCredentialGroup(env, ['APPLE_API_KEY', 'APPLE_API_KEY_ID', 'APPLE_API_ISSUER'], 'api-key')
  if (apiKey !== undefined) return apiKey
  throw new Error(
    'macOS notarization credentials are required: set APPLE_KEYCHAIN_PROFILE, the APPLE_ID trio, or the APPLE_API_KEY trio',
  )
}

/**
 * Assert that a signed, notarized macOS build cannot silently degrade into an
 * unsigned or ad-hoc one. Runs before the repository build so a missing
 * credential costs seconds rather than a full pack and bundle.
 * @param options - process inputs and the identity lookup.
 * @returns the identity and notarization mechanism the release will use.
 */
export function assertMacReleaseReady(options: MacReleaseOptions): MacReleasePlan {
  if (options.platform !== 'darwin') throw new Error('The signed macOS release must be built on macOS')

  const identities = developerIdApplications(options.listCodeSigningIdentities())
  if (identities.length === 0) {
    throw new Error('No Developer ID Application identity with its private key is available in the Keychain')
  }
  const requested = environmentValue(options.env, 'DSH_SIGN_IDENTITY')
  let identity: string
  if (requested === undefined) {
    // Picking one of several identities is the build user's decision: an
    // implicit first-match would sign a release with an unintended certificate.
    const [only, ...rest] = identities
    if (only === undefined || rest.length > 0) {
      throw new Error(`Set DSH_SIGN_IDENTITY to choose among ${String(identities.length)} Developer ID identities: ${identities.join(', ')}`)
    }
    identity = only
  } else {
    const exact = identities.filter(candidate => candidate === requested)
    // A team-qualified name (`Owner (TEAMID)`) is the form `security` prints
    // after the certificate-type prefix; anything shorter can name two
    // certificates, and choosing between those is the same decision the
    // unset-variable path refuses to make.
    const partial = identities.filter(candidate => candidate.slice(DEVELOPER_ID_PREFIX.length).trim() === requested)
    const matched = exact.length > 0 ? exact : partial
    const [only, ...ambiguous] = matched
    if (only === undefined) throw new Error(`DSH_SIGN_IDENTITY does not match a valid Keychain identity: ${requested}`)
    if (ambiguous.length > 0) {
      throw new Error(`DSH_SIGN_IDENTITY matches ${String(matched.length)} Keychain identities; name one exactly: ${matched.join(', ')}`)
    }
    identity = only
  }

  return { identity, notarization: resolveNotarizationSource(options.env) }
}

/**
 * Mount the finished image and assess the application a user would open.
 *
 * The signature and the ticket were verified on the bundle in the build tree,
 * which is not what anyone receives. This checks the copy inside the image,
 * through Gatekeeper, and it is the last step before the digest is published.
 * @param dmgPath - the stapled disk image.
 * @param env - environment for the verification tools.
 */
function verifyDistributedImage(dmgPath: string, env: NodeJS.ProcessEnv): void {
  const mountPoint = mkdtempSync(join(tmpdir(), 'dshd-verify-'))
  try {
    run('hdiutil', ['attach', dmgPath, '-nobrowse', '-readonly', '-mountpoint', mountPoint], env)
    try {
      const mounted = join(mountPoint, 'dshd.app')
      run('codesign', ['--verify', '--deep', '--strict', '--verbose=2', mounted], env)
      run('spctl', ['--assess', '--type', 'execute', '--verbose=4', mounted], env)
      assertNodeEntitlements(mounted)
    } finally {
      run('hdiutil', ['detach', mountPoint, '-force'], env)
    }
  } finally {
    rmSync(mountPoint, { recursive: true, force: true })
  }
}

/** Whether the first four bytes of `path` are a Mach-O or universal-binary magic number. */
function isMachO(path: string): boolean {
  let fd: number
  try {
    fd = openSync(path, 'r')
  } catch {
    // A dangling symbolic link cannot be signed and is not a bundle payload.
    return false
  }
  try {
    const header = Buffer.alloc(4)
    if (readSync(fd, header, 0, 4, 0) < 4) return false
    return MACH_O_MAGIC.has(header.readUInt32BE(0))
  } finally {
    closeSync(fd)
  }
}

/**
 * Every Mach-O file inside `root`, deepest path first.
 *
 * Membership is decided by the file header rather than the executable bit, so
 * native addons that ship without `+x` are signed with everything else.
 * @param root - application bundle to walk.
 * @returns absolute paths ordered so nested code is signed before its container.
 */
export function machOFiles(root: string): readonly string[] {
  const found: string[] = []
  const walk = (current: string): void => {
    for (const entry of readdirSync(current, { withFileTypes: true })) {
      const path = join(current, entry.name)
      if (entry.isSymbolicLink()) continue
      if (entry.isDirectory()) {
        walk(path)
        continue
      }
      if (entry.isFile() && isMachO(path)) found.push(path)
    }
  }
  walk(root)
  return found.sort((left, right) => right.split('/').length - left.split('/').length)
}

/**
 * Run one release step.
 *
 * A failure reports the command name and exit status only. `notarytool` takes
 * an app-specific password as an argument, so an error that echoed its
 * arguments would put that password into the terminal and into CI logs.
 * @param command - executable to run.
 * @param args - arguments, which may contain a credential.
 * @param env - environment for the child.
 * @param cwd - working directory, when it differs from this process's.
 */
function run(command: string, args: readonly string[], env: NodeJS.ProcessEnv, cwd?: string): void {
  const result = spawnSync(command, args, { stdio: 'inherit', env, ...(cwd === undefined ? {} : { cwd }) })
  if (result.error !== undefined) throw result.error
  if (result.status !== 0) throw new Error(`${command} exited with ${String(result.status)}`)
}

function capture(command: string, args: readonly string[], cwd?: string): string {
  const result = spawnSync(command, args, { encoding: 'utf8', ...(cwd === undefined ? {} : { cwd }) })
  if (result.error !== undefined) throw result.error
  if (result.status !== 0) throw new Error(`${command} ${args.join(' ')} exited with ${String(result.status)}: ${result.stderr.trim()}`)
  return result.stdout
}

function listCodeSigningIdentities(): string {
  return capture('security', ['find-identity', '-v', '-p', 'codesigning'])
}

/**
 * Assert the embedded Node runtime kept its exemptions through the bundle seal.
 *
 * The entitlements are granted per file, so any later signing pass over the
 * bundle can silently replace them, and the resulting application starts and
 * then fails only when V8 first compiles. Reading them back turns that into a
 * release-time failure.
 * @param appPath - the signed application bundle.
 */
function assertNodeEntitlements(appPath: string): void {
  const entitlements = capture('codesign', ['--display', '--entitlements', '-', '--xml', `${appPath}${NODE_RUNTIME_SUFFIX}`])
  if (!entitlements.includes(NODE_REQUIRED_ENTITLEMENT)) {
    throw new Error(`the embedded Node runtime lost ${NODE_REQUIRED_ENTITLEMENT} during signing`)
  }
}

/** One credential the selected group promised, read at the point it is spent. */
function credential(env: NodeJS.ProcessEnv, name: string): string {
  const value = environmentValue(env, name)
  if (value === undefined) throw new Error(`macOS notarization credential ${name} is not set`)
  return value
}

/** The `notarytool` arguments for the selected credential group. */
function notarizationArguments(source: NotarizationSource, env: NodeJS.ProcessEnv): readonly string[] {
  switch (source) {
    case 'keychain-profile':
      return ['--keychain-profile', credential(env, 'APPLE_KEYCHAIN_PROFILE')]
    case 'apple-id':
      return [
        '--apple-id', credential(env, 'APPLE_ID'),
        '--password', credential(env, 'APPLE_APP_SPECIFIC_PASSWORD'),
        '--team-id', credential(env, 'APPLE_TEAM_ID'),
      ]
    case 'api-key':
      return [
        '--key', credential(env, 'APPLE_API_KEY'),
        '--key-id', credential(env, 'APPLE_API_KEY_ID'),
        '--issuer', credential(env, 'APPLE_API_ISSUER'),
      ]
  }
}

/**
 * Release inputs that carry no secret but still must not steer a build.
 * The remaining Apple variables all match {@link CREDENTIAL_PATTERN}.
 */
const RELEASE_VARIABLES = ['APPLE_ID', 'APPLE_TEAM_ID', 'APPLE_API_ISSUER', 'DSH_SIGN_IDENTITY'] as const
/** Credential-shaped names, per the repository's scrubbed-environment rule. */
const CREDENTIAL_PATTERN = /KEY|SECRET|TOKEN|PASSWORD/i

/**
 * The environment for build and pack subprocesses.
 *
 * The release runs `pnpm build` and a sidecar pack whose `npm install` executes
 * dependency lifecycle scripts. Those steps need no credential of any kind, so
 * they receive none: every credential-shaped name is dropped, not just the
 * Apple ones this file names ([defensive patterns](../../docs/defensive-patterns.md)).
 * `codesign` and `notarytool` are the only steps that see the real environment.
 * @param env - environment the release runs under.
 * @returns a copy without credential-shaped or release-steering variables.
 */
export function buildEnvironment(env: NodeJS.ProcessEnv): NodeJS.ProcessEnv {
  const named = new Set<string>(RELEASE_VARIABLES)
  return Object.fromEntries(
    Object.entries(env).filter(([name]) => !named.has(name) && !CREDENTIAL_PATTERN.test(name)),
  )
}

/** Updater signing inputs, which only the bundle step may see. */
const UPDATER_SIGNING_VARIABLES = ['TAURI_SIGNING_PRIVATE_KEY', 'TAURI_SIGNING_PRIVATE_KEY_PASSWORD'] as const

/**
 * The environment for the bundle step: {@link buildEnvironment} plus the
 * updater signing key.
 *
 * `tauri build` signs the updater artifact as it produces it, so this one step
 * cannot run under the scrubbed environment the other steps use. Widening
 * {@link buildEnvironment} instead would hand the key to `pnpm build` and to
 * the sidecar pack, whose `npm install` reaches dependency code; `bundleApp`
 * runs only `tauri build` and installs nothing, so the exposure stops here.
 *
 * @param env - environment the release runs under.
 * @returns the build environment with the signing variables restored.
 */
export function bundleEnvironment(env: NodeJS.ProcessEnv): NodeJS.ProcessEnv {
  const bundle = buildEnvironment(env)
  for (const name of UPDATER_SIGNING_VARIABLES) {
    const value = env[name]
    if (value !== undefined) bundle[name] = value
  }
  return bundle
}

/** Build, sign, notarize and staple the macOS application, then verify the result. */
export function releaseDesktopMac(): void {
  const plan = assertMacReleaseReady({
    env: process.env,
    platform: process.platform,
    listCodeSigningIdentities,
  })
  console.log(`macOS release preflight passed: ${plan.identity}; notarization via ${plan.notarization}`)

  const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
  const desktopRoot = join(repoRoot, 'apps/desktop')
  const nodeEntitlements = join(desktopRoot, 'src-tauri/entitlements.node.plist')
  const manifest: unknown = JSON.parse(readFileSync(join(desktopRoot, 'package.json'), 'utf8'))
  const version = isRecord(manifest) && typeof manifest.version === 'string' ? manifest.version : undefined
  if (version === undefined) throw new Error('apps/desktop/package.json must declare a version')

  const buildEnv = buildEnvironment(process.env)
  run('pnpm', ['--workspace-root', 'run', 'build'], buildEnv, repoRoot)
  run('node', ['scripts/pack-sidecar.mjs'], buildEnv, desktopRoot)
  // The bundle step owns the source-path remap every build path needs; see
  // `bundleApp` in pack-sidecar.mjs.
  run('node', ['scripts/pack-sidecar.mjs', 'bundle'], bundleEnvironment(process.env), desktopRoot)
  run('node', ['scripts/pack-sidecar.mjs', 'embed'], buildEnv, desktopRoot)

  // The bundle step owns where the bundle lands, because the explicit build
  // target puts it under a triple-named directory.
  const [appPath] = capture('node', ['scripts/pack-sidecar.mjs', 'app-path'], desktopRoot).trim().split('\n')
  if (appPath === undefined || appPath === '') throw new Error('pack-sidecar did not report the application bundle path')
  const sign = (target: string, grants: string | undefined): void => {
    run('codesign', [
      '--force', '--options', 'runtime', '--timestamp',
      ...(grants === undefined ? [] : ['--entitlements', grants]),
      '--sign', plan.identity, target,
    ], buildEnv)
  }
  // Attribution obligations attach to the distributed artifact, not to the
  // repository, so a bundle without these files discloses nothing to the person
  // who received it.
  for (const required of ['LICENSE', 'THIRD_PARTY_NOTICES.md']) {
    if (!existsSync(join(appPath, 'Contents/Resources', required))) {
      throw new Error(`the application bundle is missing ${required}; check the tauri.conf.json resources`)
    }
  }
  const nested = machOFiles(appPath)
  console.log(`signing ${String(nested.length)} Mach-O file(s) inside ${appPath}`)
  // Innermost first, then the bundle. `--deep` is deliberately absent: it
  // re-signs everything it finds with the arguments of the outer invocation,
  // which would replace the Node runtime's entitlements with the application's.
  for (const file of nested) sign(file, file.endsWith(NODE_RUNTIME_SUFFIX) ? nodeEntitlements : undefined)
  sign(appPath, undefined)
  run('codesign', ['--verify', '--deep', '--strict', '--verbose=2', appPath], buildEnv)
  assertNodeEntitlements(appPath)

  const dmgPath = join(desktopRoot, `dist/dshd-${version}-${process.arch}.dmg`)
  const stage = mkdtempSync(join(tmpdir(), 'dshd-dmg-'))
  try {
    cpSync(appPath, join(stage, 'dshd.app'), { recursive: true, verbatimSymlinks: true })
    symlinkSync('/Applications', join(stage, 'Applications'))
    rmSync(dmgPath, { force: true })
    run('mkdir', ['-p', dirname(dmgPath)], buildEnv)
    run('hdiutil', ['create', '-volname', VOLUME_NAME, '-srcfolder', stage, '-ov', '-format', 'UDZO', dmgPath], buildEnv)
  } finally {
    rmSync(stage, { recursive: true, force: true })
  }
  sign(dmgPath, undefined)

  run('xcrun', ['notarytool', 'submit', dmgPath, ...notarizationArguments(plan.notarization, process.env), '--wait'], process.env)
  run('xcrun', ['stapler', 'staple', dmgPath], buildEnv)
  run('xcrun', ['stapler', 'validate', dmgPath], buildEnv)
  verifyDistributedImage(dmgPath, buildEnv)
  const digest = capture('shasum', ['-a', '256', dmgPath]).trim().split(/\s+/)[0] ?? ''
  writeFileSync(join(dirname(dmgPath), 'SHA256SUMS.txt'), `${digest}  ${basename(dmgPath)}\n`)
  console.log(`macOS release ready: ${dmgPath}`)
  console.log(`sha256: ${digest}`)
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null
}

const invokedPath = process.argv[1]
if (invokedPath !== undefined && resolve(invokedPath) === fileURLToPath(import.meta.url)) {
  try {
    releaseDesktopMac()
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error))
    process.exitCode = 1
  }
}
