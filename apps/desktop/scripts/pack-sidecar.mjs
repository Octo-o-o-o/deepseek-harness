#!/usr/bin/env node
/**
 * Build the desktop sidecar: production `pnpm deploy` of @deepseek-ai/dsh,
 * a pinned Node runtime, and a PATH-stripped boot self-check.
 *
 * Zero npm dependencies. Run from the repository root or apps/desktop.
 */

import { createHash } from 'node:crypto'
import { createWriteStream } from 'node:fs'
import { existsSync } from 'node:fs'
import { mkdir, readFile, writeFile, rm, chmod, cp, readdir, lstat, readlink, rename, symlink } from 'node:fs/promises'
import { homedir, tmpdir } from 'node:os'
import { dirname, join, relative, resolve } from 'node:path'
import { pipeline } from 'node:stream/promises'
import { fileURLToPath } from 'node:url'
import { spawn } from 'node:child_process'

const NODE_VERSION = '24.19.0'
const NODE_ARCHIVES = {
  'darwin-arm64': `node-v${NODE_VERSION}-darwin-arm64.tar.gz`,
  'darwin-x64': `node-v${NODE_VERSION}-darwin-x64.tar.gz`,
  'win32-x64': `node-v${NODE_VERSION}-win-x64.zip`,
}
/**
 * Digest of each archive, from nodejs.org's SHASUMS256.txt for this version.
 *
 * Kept here rather than downloaded with the archive: a digest fetched from the
 * same host at the same moment confirms the transfer, not the publisher, and it
 * is the runtime a signed application ships. Update both together, and only
 * from an official release.
 */
const NODE_DIGESTS = {
  'darwin-arm64': '8294b7aa9b03997481c06babf1e8b270c859358f27da57a11509afe537ac381d',
  'darwin-x64': 'd1b5e999db158c62fe8f7267a4476b035d8bd93b1a605bac24a3f0dd166e3316',
  'win32-x64': '57f71ab3652e797d84acddc79c81cc9ff1c6ddb2a1974cdb83f00fee9bff4c73',
}

/** Extensions a bundler can emit a source path into. */
const TEXT_PAYLOAD = /\.(?:js|mjs|cjs|jsx|ts|tsx|json|map|css|html|htm|txt|md)$/i

const here = dirname(fileURLToPath(import.meta.url))
const desktopRoot = resolve(here, '..')
const repoRoot = resolve(desktopRoot, '../..')
const sidecarRoot = join(desktopRoot, 'sidecar')
const cacheDir = join(sidecarRoot, 'cache')
const distDir = join(sidecarRoot, 'dist')
const appDir = join(distDir, 'app')
const binDir = join(distDir, 'bin')

/**
 * Remove a directory tree.
 *
 * The payload is a deep `node_modules`, and removing one on macOS intermittently
 * fails with `ENOTEMPTY` while the file system settles. `fs.rm` does not retry
 * unless asked to, so a pack could fail before it started any work.
 * @param {string} path
 * @returns {Promise<void>}
 */
function removeTree(path) {
  return rm(path, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 })
}

/**
 * @param {string} command
 * @param {string[]} args
 * @param {{ cwd?: string, env?: NodeJS.ProcessEnv }} [opts]
 * @returns {Promise<void>}
 */
function run(command, args, opts = {}) {
  return new Promise((resolveRun, reject) => {
    const child = spawn(command, args, {
      cwd: opts.cwd ?? repoRoot,
      env: opts.env ?? process.env,
      stdio: 'inherit',
    })
    child.on('error', reject)
    child.on('exit', (code) => {
      if (code === 0) resolveRun()
      else reject(new Error(`${command} ${args.join(' ')} exited ${String(code)}`))
    })
  })
}

/**
 * @param {string} command
 * @param {string[]} args
 * @param {{ cwd?: string, env?: NodeJS.ProcessEnv }} [opts]
 * @returns {Promise<{ code: number | null, stdout: string, stderr: string }>}
 */
function capture(command, args, opts = {}) {
  return new Promise((resolveRun, reject) => {
    const child = spawn(command, args, {
      cwd: opts.cwd ?? repoRoot,
      env: opts.env ?? process.env,
      stdio: ['ignore', 'pipe', 'pipe'],
    })
    let stdout = ''
    let stderr = ''
    child.stdout.on('data', chunk => { stdout += String(chunk) })
    child.stderr.on('data', chunk => { stderr += String(chunk) })
    child.on('error', reject)
    child.on('exit', code => { resolveRun({ code, stdout, stderr }) })
  })
}

/**
 * @param {string} url
 * @param {string} dest
 * @returns {Promise<void>}
 */
async function download(url, dest) {
  const response = await fetch(url)
  if (!response.ok || response.body === null) {
    throw new Error(`download failed ${url}: HTTP ${String(response.status)}`)
  }
  await pipeline(response.body, createWriteStream(dest))
}

/**
 * @param {string} file
 * @returns {Promise<string>}
 */
async function sha256File(file) {
  const hash = createHash('sha256')
  hash.update(await readFile(file))
  return hash.digest('hex')
}

/**
 * @returns {keyof typeof NODE_ARCHIVES}
 */
function hostTriple() {
  const key = `${process.platform}-${process.arch}`
  if (key === 'darwin-arm64' || key === 'darwin-x64' || key === 'win32-x64') return key
  throw new Error(`unsupported pack host ${key}`)
}


/**
 * Restore the executable bit on helper binaries shipped inside `prebuilds`.
 *
 * A package that ships a helper program next to its addon normally makes it
 * executable from its install script, and this install runs with
 * `--ignore-scripts`. node-pty's `spawn-helper` is the one the payload needs:
 * without the bit, `node-pty` loads and then every terminal fails with
 * `posix_spawnp failed`. Addons themselves (`.node`) are loaded, not executed,
 * so they are left alone.
 * @param {string} dir - installed `node_modules`.
 */
async function restorePrebuildHelpers(dir) {
  let fixed = 0
  const walk = async (current, insidePrebuilds) => {
    for (const entry of await readdir(current, { withFileTypes: true })) {
      const path = join(current, entry.name)
      if (entry.isSymbolicLink()) continue
      if (entry.isDirectory()) {
        await walk(path, insidePrebuilds || entry.name === 'prebuilds')
        continue
      }
      if (!insidePrebuilds || !entry.isFile() || entry.name.endsWith('.node')) continue
      const header = (await readFile(path)).subarray(0, 4)
      if (header.length < 4) continue
      const magic = header.readUInt32BE(0)
      if (!MACH_O_MAGIC.has(magic)) continue
      const mode = (await lstat(path)).mode
      if ((mode & 0o111) !== 0) continue
      await chmod(path, (mode & 0o777) | 0o755)
      fixed += 1
    }
  }
  await walk(dir, false)
  if (fixed > 0) console.log(`pack-sidecar: restored the executable bit on ${String(fixed)} prebuilt helper(s)`)
}

/** Mach-O and universal-binary magic numbers, in both byte orders. */
const MACH_O_MAGIC = new Set([0xfeedface, 0xfeedfacf, 0xcefaedfe, 0xcffaedfe, 0xcafebabe, 0xbebafeca])

/** Recorded external package versions and the CLI version this payload carries. */
const manifestPath = join(desktopRoot, 'payload-manifest.json')

/**
 * Every package in an installed tree, as `name` → `version`.
 * @param {string} nodeModules
 * @returns {Promise<Record<string, string>>}
 */
async function installedPackages(nodeModules) {
  /** @type {Record<string, string>} */
  const found = {}
  const walk = async (dir, scope) => {
    let entries
    try {
      entries = await readdir(dir, { withFileTypes: true })
    } catch {
      return
    }
    for (const entry of entries) {
      if (!entry.isDirectory() && !entry.isSymbolicLink()) continue
      if (entry.name.startsWith('.')) continue
      if (scope === undefined && entry.name.startsWith('@')) {
        await walk(join(dir, entry.name), entry.name)
        continue
      }
      const path = join(dir, entry.name)
      try {
        const manifest = JSON.parse(await readFile(join(path, 'package.json'), 'utf8'))
        if (typeof manifest.name === 'string' && typeof manifest.version === 'string') {
          found[manifest.name] = manifest.version
        }
      } catch {
        // A directory without a readable package.json is not an installed package.
      }
      await walk(join(path, 'node_modules'), undefined)
    }
  }
  await walk(nodeModules, undefined)
  return found
}

/**
 * The payload's own packages, which are built from this checkout and therefore
 * carry the repository version rather than a recorded one.
 * @param {string} name
 * @returns {boolean}
 */
function isLocalPackage(name) {
  return name.startsWith('@deepseek-ai/')
}

/**
 * Compare the installed external packages against `payload-manifest.json`.
 *
 * The install resolves ranges (`commander ^15.0.0`) against the registry, so the
 * same commit packed on two days can ship different code. Recording the resolved
 * versions turns that into a build failure instead of a silent difference in a
 * signed artifact. `node scripts/pack-sidecar.mjs manifest` records the current
 * resolution deliberately.
 * @param {string} nodeModules
 * @param {string} cliVersion
 */
async function assertPayloadManifest(nodeModules, cliVersion) {
  const installed = await installedPackages(nodeModules)
  const external = Object.fromEntries(
    Object.entries(installed).filter(([name]) => !isLocalPackage(name)).sort(([a], [b]) => (a < b ? -1 : 1)),
  )
  const record = { cli: cliVersion, node: NODE_VERSION, packages: external }
  if (process.argv[2] === 'manifest') {
    await writeFile(manifestPath, JSON.stringify(record, null, 2) + '\n')
    console.log(`pack-sidecar: recorded ${String(Object.keys(external).length)} external package(s) in payload-manifest.json`)
    return
  }
  let recorded
  try {
    recorded = JSON.parse(await readFile(manifestPath, 'utf8'))
  } catch {
    throw new Error('pack-sidecar: payload-manifest.json is missing; run `node scripts/pack-sidecar.mjs manifest`')
  }
  const drift = []
  if (recorded.node !== NODE_VERSION) drift.push(`node ${String(recorded.node)} -> ${NODE_VERSION}`)
  if (recorded.cli !== cliVersion) drift.push(`@deepseek-ai/dsh ${String(recorded.cli)} -> ${cliVersion}`)
  for (const [name, version] of Object.entries(external)) {
    const was = recorded.packages?.[name]
    if (was === undefined) drift.push(`+ ${name}@${version}`)
    else if (was !== version) drift.push(`${name} ${was} -> ${version}`)
  }
  for (const name of Object.keys(recorded.packages ?? {})) {
    if (external[name] === undefined) drift.push(`- ${name}`)
  }
  if (drift.length > 0) {
    throw new Error(
      `pack-sidecar: payload differs from payload-manifest.json (${String(drift.length)}): ${drift.slice(0, 10).join(', ')}` +
        '\n  Re-run with the `manifest` step to accept the new resolution.',
    )
  }
  console.log(`pack-sidecar: payload matches payload-manifest.json (${String(Object.keys(external).length)} external package(s))`)
}

/**
 * Refuse a build whose three desktop version declarations disagree.
 *
 * The bundle takes `CFBundleShortVersionString` from `tauri.conf.json`, the DMG
 * name from `package.json`, and the crate version from `Cargo.toml`; nothing
 * else compares them.
 * @returns {Promise<string>} the agreed version.
 */
async function assertDesktopVersion() {
  const pkg = JSON.parse(await readFile(join(desktopRoot, 'package.json'), 'utf8')).version
  const conf = JSON.parse(await readFile(join(desktopRoot, 'src-tauri/tauri.conf.json'), 'utf8')).version
  const cargo = /^version = "([^"]+)"$/m.exec(await readFile(join(desktopRoot, 'src-tauri/Cargo.toml'), 'utf8'))?.[1]
  if (pkg !== conf || pkg !== cargo) {
    throw new Error(`pack-sidecar: desktop versions disagree: package.json ${String(pkg)}, tauri.conf.json ${String(conf)}, Cargo.toml ${String(cargo)}`)
  }
  return pkg
}

/**
 * Assemble the payload from the repository's official release tarballs plus
 * an offline npm install. pnpm deploy keeps vendor packages as workspace
 * symlinks and an absolute-link .pnpm zoo, both of which break inside an app
 * bundle; npm's flat install carries every package as real directories.
 */
async function deployApp() {
  await assertDesktopVersion()
  await removeTree(appDir)
  await mkdir(distDir, { recursive: true })
  const packRoot = join(sidecarRoot, 'pack')
  await removeTree(packRoot)
  const packEnv = { ...process.env, CI: 'true' }
  const tsx = join(repoRoot, 'node_modules/.bin/tsx')
  for (const family of ['vendor', 'dsh']) {
    await run(tsx, ['scripts/release/pack.ts', '--family', family, '--out', join(packRoot, family)], { env: packEnv })
  }
  const manifests = new Map()
  const tarballBy = new Map()
  for (const family of ['vendor', 'dsh']) {
    for (const filename of await readdir(join(packRoot, family))) {
      if (!filename.endsWith('.tgz')) continue
      const tarball = join(packRoot, family, filename)
      const manifest = JSON.parse((await capture('tar', ['-xOf', tarball, 'package/package.json'])).stdout)
      manifests.set(manifest.name, manifest)
      tarballBy.set(manifest.name, tarball)
    }
  }
  // Install only the CLI workspace closure: the release family also packs demos,
  // test support, and build tooling whose toolchains (typescript/vitest/rolldown)
  // must never ship in the desktop payload.
  const LOCAL = /^@deepseek-ai\/(dsh-|cordis|schemastery|cosmokit|node-addon-)/
  const queue = ['@deepseek-ai/dsh']
  const closure = new Set()
  while (queue.length > 0) {
    const name = queue.pop()
    if (name === undefined || closure.has(name)) continue
    closure.add(name)
    const manifest = manifests.get(name)
    if (manifest === undefined) continue
    for (const section of ['dependencies', 'peerDependencies', 'optionalDependencies']) {
      for (const dep of Object.keys(manifest[section] ?? {})) {
        if (LOCAL.test(dep) && !closure.has(dep)) queue.push(dep)
      }
    }
  }
  const deps = {}
  for (const name of closure) {
    const tarball = tarballBy.get(name)
    if (tarball !== undefined) deps[name] = 'file:' + tarball
  }
  const consumer = join(sidecarRoot, 'consumer')
  await removeTree(consumer)
  await mkdir(consumer, { recursive: true })
  await writeFile(
    join(consumer, 'package.json'),
    JSON.stringify({ name: 'dshd-sidecar', version: '0.0.0', private: true, dependencies: deps }, null, 2) + '\n',
  )
  // Optional dependencies stay: koffi and the native prebuild families load from them.
  //
  // `--ignore-scripts`: this install runs on the machine that holds the
  // Developer ID identity, and a dependency's install script would execute
  // there with that machine's privileges. The payload needs no build step —
  // every native package it carries ships prebuilt binaries — so the scripts
  // have nothing to contribute. `assertPayloadManifest` then rejects a tree
  // whose external package versions are not the recorded ones.
  await run('npm', ['install', '--no-audit', '--no-fund', '--package-lock=false', '--ignore-scripts'], { cwd: consumer, env: packEnv })
  const required = [
    join(consumer, 'node_modules/@deepseek-ai/dsh/lib/bin.js'),
    join(consumer, 'node_modules/@deepseek-ai/dsh-web-frontend/dist/index.html'),
    join(consumer, 'node_modules/@deepseek-ai/dsh-web-app/package.json'),
    join(consumer, 'node_modules/@deepseek-ai/dsh-base/package.json'),
  ]
  for (const path of required) {
    try {
      await readFile(path)
    } catch {
      throw new Error('deploy missing ' + path)
    }
  }
  await assertPayloadManifest(join(consumer, 'node_modules'), manifests.get('@deepseek-ai/dsh')?.version ?? 'unknown')
  await cp(join(consumer, 'node_modules'), join(appDir, 'node_modules'), { recursive: true })
  // Lay the CLI package contents at the payload root: the shell expects app/lib/bin.js.
  await cp(join(consumer, 'node_modules/@deepseek-ai/dsh'), appDir, { recursive: true })
  await restorePrebuildHelpers(join(appDir, 'node_modules'))
  await stripAbsoluteSymlinks(join(appDir, 'node_modules'))
  await pruneNonHostArtifacts(appDir)
  await stripDevArtifacts(join(appDir, 'node_modules'))
  await stripDevArtifacts(appDir)
  await scrubCheckoutPaths(appDir)
  await removeTree(consumer)
}

async function pathExists(path) {
  try {
    await lstat(path)
    return true
  } catch {
    return false
  }
}

async function installNodeRuntime() {
  const triple = hostTriple()
  const archive = NODE_ARCHIVES[triple]
  const url = `https://nodejs.org/dist/v${NODE_VERSION}/${archive}`
  const expected = NODE_DIGESTS[triple]
  await mkdir(cacheDir, { recursive: true })
  const archivePath = join(cacheDir, archive)
  // An interrupted download used to leave a short file at the final path, and
  // every later run then failed the digest check until someone cleared the
  // cache by hand. Fetch to a temporary name and rename only what verifies.
  if (!(await exists(archivePath)) || (await sha256File(archivePath)) !== expected) {
    await rm(archivePath, { force: true })
    const partial = join(cacheDir, `${archive}.${String(process.pid)}.part`)
    await download(url, partial)
    const actual = await sha256File(partial)
    if (actual !== expected) {
      await rm(partial, { force: true })
      throw new Error(`pack-sidecar: sha256 mismatch for ${archive}: expected ${expected}, got ${actual}`)
    }
    await rename(partial, archivePath)
  }
  await mkdir(binDir, { recursive: true })
  if (triple.startsWith('win32')) {
    const extractRoot = join(tmpdir(), `dsh-node-${NODE_VERSION}-${process.pid}`)
    await rm(extractRoot, { recursive: true, force: true })
    await mkdir(extractRoot, { recursive: true })
    await run('tar', ['-xf', archivePath, '-C', extractRoot])
    const extracted = join(extractRoot, archive.replace(/\.zip$/, ''), 'node.exe')
    await cp(extracted, join(binDir, 'node.exe'))
    await rm(extractRoot, { recursive: true, force: true })
    return
  }
  const extractRoot = join(tmpdir(), `dsh-node-${NODE_VERSION}-${process.pid}`)
  await rm(extractRoot, { recursive: true, force: true })
  await mkdir(extractRoot, { recursive: true })
  await run('tar', ['-xzf', archivePath, '-C', extractRoot])
  const extracted = join(extractRoot, archive.replace(/\.tar\.gz$/, ''), 'bin', 'node')
  await cp(extracted, join(binDir, 'node'))
  await chmod(join(binDir, 'node'), 0o755)
  await rm(extractRoot, { recursive: true, force: true })
}

/**
 * @param {string} path
 * @returns {Promise<boolean>}
 */
async function exists(path) {
  try {
    await readFile(path)
    return true
  } catch {
    return false
  }
}

/**
 * Load the payload's native packages with the bundled runtime.
 *
 * The install runs with `--ignore-scripts`, so every native package has to work
 * from a prebuilt binary, and the prune step keeps only the host's. Starting the
 * web server proves neither: it opens no terminal and loads no addon. This
 * opens a pseudo-terminal and the bundled `sharp` and `koffi` addons with the
 * runtime that ships, plus the runtime's own SQLite, which is where a payload
 * built for another architecture fails.
 */
async function checkNativeModules() {
  const node = process.platform === 'win32' ? join(binDir, 'node.exe') : join(binDir, 'node')
  const source = `
    const pty = require('node-pty')
    const shell = process.platform === 'win32' ? 'cmd.exe' : '/bin/sh'
    const term = pty.spawn(shell, [], { name: 'xterm-color', cols: 80, rows: 24 })
    if (typeof term.pid !== 'number') throw new Error('node-pty did not report a pid')
    term.kill()
    const { DatabaseSync } = require('node:sqlite')
    const db = new DatabaseSync(':memory:')
    db.exec('create table probe (id integer primary key)')
    db.close()
    require('sharp')
    require('koffi')
    console.log('native ok')
  `
  const { code, stdout, stderr } = await capture(node, ['-e', source], {
    cwd: appDir,
    env: { ...process.env, NODE_PATH: join(appDir, 'node_modules') },
  })
  if (code !== 0 || !stdout.includes('native ok')) {
    throw new Error(`pack-sidecar: native module probe failed (${String(code)}): ${stderr.trim() || stdout.trim()}`)
  }
  console.log('pack-sidecar: node-pty, sharp, koffi, and node:sqlite work with the bundled runtime')
}

async function selfCheck() {
  await checkNativeModules()
  const node = process.platform === 'win32' ? join(binDir, 'node.exe') : join(binDir, 'node')
  const binJs = join(appDir, 'lib/bin.js')
  const home = join(tmpdir(), `dsh-pack-check-${String(process.pid)}`)
  await rm(home, { recursive: true, force: true })
  await mkdir(home, { recursive: true })
  const child = spawn(node, [binJs, 'web', '--port', '0', '--host', '127.0.0.1'], {
    cwd: home,
    env: {
      PATH: '/usr/bin:/bin:/usr/sbin:/sbin',
      DSH_HOME: home,
      HOME: home,
      NODE_PATH: join(appDir, 'node_modules'),
    },
    stdio: ['ignore', 'pipe', 'pipe'],
  })
  let stdout = ''
  child.stdout.on('data', chunk => { stdout += String(chunk) })
  child.stderr.on('data', chunk => { stdout += String(chunk) })
  const port = await new Promise((resolvePort, reject) => {
    let settled = false
    const timer = setTimeout(() => {
      if (settled) return
      settled = true
      child.kill('SIGTERM')
      reject(new Error('sidecar self-check timed out waiting for ready line'))
    }, 15_000)
    const consider = () => {
      const match = /dsh web: http:\/\/127\.0\.0\.1:(\d+)/.exec(stdout)
      if (match === null || settled) return
      settled = true
      clearTimeout(timer)
      resolvePort(match[1])
    }
    child.stdout.on('data', consider)
    child.stderr.on('data', consider)
    child.on('exit', code => {
      if (settled) return
      settled = true
      clearTimeout(timer)
      reject(new Error(`sidecar exited before ready: ${String(code)}\n${stdout}`))
    })
  })
  console.log(`pack-sidecar: ready http://127.0.0.1:${port}`)
  const response = await fetch(`http://127.0.0.1:${port}/`)
  const body = await response.text()
  if (response.status !== 200 || !body.includes('__DSH_BOOT__')) {
    child.kill('SIGTERM')
    throw new Error(`self-check GET / failed: HTTP ${String(response.status)}`)
  }
  console.log('pack-sidecar: GET / 200 __DSH_BOOT__')
  const finished = new Promise(resolveExit => {
    child.on('exit', (code, signal) => { resolveExit({ code, signal }) })
  })
  child.kill('SIGTERM')
  const result = await Promise.race([
    finished,
    new Promise(resolveTimeout => {
      setTimeout(() => { child.kill('SIGKILL'); resolveTimeout({ code: null, signal: 'SIGKILL' }) }, 5000)
    }),
  ])
  await rm(home, { recursive: true, force: true })
  if (result.code !== 0 && result.signal !== 'SIGTERM') {
    throw new Error(`self-check SIGTERM exit was ${String(result.code)}/${String(result.signal)}`)
  }
  console.log(`pack-sidecar: SIGTERM ${String(result.code)}/${String(result.signal)}`)
}

/**
 * Build the application bundle.
 *
 * rustc records each compilation unit's absolute source path in the binary,
 * which on any build machine means a path under the build user's home;
 * `assertNoBuildMachinePaths` refuses a bundle that carries one. The remap is
 * therefore part of building the bundle rather than a step of the signed
 * release alone, so the package script, CI, and the release all produce the
 * same executable. The encoded form carries paths containing spaces, which the
 * whitespace-split `RUSTFLAGS` cannot.
 *
 * The flag also reaches proc-macro crates, which is why the release profile
 * leaves host units unstripped; see `[profile.release.build-override]` in
 * `src-tauri/Cargo.toml`.
 */
async function bundleApp() {
  await run('pnpm', ['exec', 'tauri', 'build', '--bundles', 'app'], {
    cwd: desktopRoot,
    env: {
      ...process.env,
      CARGO_ENCODED_RUSTFLAGS: `--remap-path-prefix=${homedir()}=/build`,
    },
  })
}

/**
 * Where `bundleApp` leaves the application bundle; the signed release and the
 * smoke script both have to find it.
 * @returns {string}
 */
function appBundlePath() {
  return join(desktopRoot, 'src-tauri/target/release/bundle/macos/dshd.app')
}

/**
 * Tauri's resource copy drops directory symlinks, so the bundled
 * `node_modules` would keep `.pnpm` and lose every hoisted package.
 * Re-copy with `cp -a` after `tauri build` so the .app matches the
 * self-checked tree.
 */
async function embedIntoApp() {
  const appPath = appBundlePath()
  const destRoot = join(appPath, 'Contents/Resources')
  if (!(await pathExists(join(appPath, 'Contents/MacOS/dshd')))) {
    throw new Error(`embed: missing app bundle at ${appPath}`)
  }
  const destBin = join(destRoot, 'bin')
  const destApp = join(destRoot, 'app')
  await rm(destBin, { recursive: true, force: true })
  await rm(destApp, { recursive: true, force: true })
  await mkdir(destRoot, { recursive: true })
  await run('cp', ['-a', binDir, destBin])
  await run('cp', ['-a', appDir, destApp])
  await relativizeSymlinks(destApp)
  await relativizeSymlinks(appDir)
  await stripAbsoluteSymlinks(join(destApp, 'node_modules'))
  const node = join(destBin, 'node')
  const binJs = join(destApp, 'lib/bin.js')
  const boot = join(destApp, 'node_modules/@deepseek-ai/dsh-app-boot/package.json')
  for (const path of [node, binJs, boot]) {
    if (!(await pathExists(path))) throw new Error(`embed missing ${path}`)
  }
  await assertNoBuildMachinePaths(appPath)
}

/**
 * Remove platform prebuilds and SDK natives the desktop runtime never loads.
 * The Claude agent SDK alone carries a 256MB darwin binary through its optional
 * platform packages, and node-pty ships prebuilds for every OS.
 */
async function pruneNonHostArtifacts(dir) {
  const remove = async (rel) => {
    const path = join(dir, rel)
    if (await pathExists(path)) await rm(path, { recursive: true, force: true })
  }
  await remove('node_modules/@anthropic-ai/claude-agent-sdk-darwin-arm64')
  await remove('node_modules/@anthropic-ai/claude-agent-sdk-darwin-x64')
  await remove('node_modules/@anthropic-ai/claude-agent-sdk-win32-x64')
  await remove('node_modules/@anthropic-ai/claude-agent-sdk-linux-x64')
  await remove('node_modules/@img/sharp-wasm32')
  await remove('node_modules/node-pty/prebuilds/darwin-x64')
  await remove('node_modules/node-pty/prebuilds/win32-x64')
  await remove('node_modules/node-pty/prebuilds/linux-x64')
  console.log('pack-sidecar: pruned non-host native artifacts')
}

/**
 * Strip sourcemaps, type declarations, and build metadata from the payload:
 * the desktop runtime imports compiled JS only. Licenses and package.json
 * files stay for attribution and module resolution.
 */
async function stripDevArtifacts(dir) {
  let bytes = 0
  const walk = async (current) => {
    for (const entry of await readdir(current, { withFileTypes: true })) {
      const path = join(current, entry.name)
      if (entry.isDirectory()) {
        if (entry.name === '.cache') {
          try {
            const size = await dirSize(path)
            await rm(path, { recursive: true, force: true })
            bytes += size
          } catch { /* keep walking on unreadable caches */ }
          continue
        }
        await walk(path)
        continue
      }
      if (!entry.isFile()) continue
      if (!/\.map$|\.d\.ts$|\.tsbuildinfo$/.test(entry.name)) continue
      try {
        bytes += (await lstat(path)).size
        await rm(path)
      } catch { /* keep unreadable entries */ }
    }
  }
  await walk(dir)
  if (bytes > 0) console.log('pack-sidecar: stripped ' + (bytes / 1048576).toFixed(1) + ' MB of dev artifacts')
}

/**
 * Recursive directory byte size.
 */
async function dirSize(dir) {
  let total = 0
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name)
    if (entry.isDirectory()) total += await dirSize(path)
    else if (entry.isFile()) total += (await lstat(path)).size
  }
  return total
}

/**
 * Neutralize build-machine checkout paths baked into client bundles.
 * The tsdown CSS virtual-id embeds the absolute source path in every UI
 * bundle; shipping that leaks the build machine username and layout for no
 * runtime benefit.
 *
 * The prefix is derived from the checkout being packed rather than written
 * literally: a literal matches one machine and one directory layout, and a
 * checkout it does not describe scrubs nothing while still reporting success.
 *
 * Rewriting is best-effort across every text payload and both escape forms a
 * bundler emits. It is not the guarantee: `assertNoBuildMachinePaths` reads the
 * finished bundle back, including compiled binaries this pass cannot rewrite.
 */
async function scrubCheckoutPaths(dir) {
  const neutral = 'deepseek-harness/'
  const prefix = `${repoRoot}/`
  // A bundler may emit the path raw, JSON-escaped, or percent-encoded inside a
  // sourceURL; each spelling is a separate literal in the emitted text.
  const spellings = [
    [prefix, neutral],
    [prefix.replaceAll('/', '\\/'), neutral.replaceAll('/', '\\/')],
    [encodeURI(prefix), encodeURI(neutral)],
  ]
  let files = 0
  const walk = async (current) => {
    for (const entry of await readdir(current, { withFileTypes: true })) {
      const path = join(current, entry.name)
      if (entry.isDirectory()) {
        await walk(path)
        continue
      }
      if (!entry.isFile() || !TEXT_PAYLOAD.test(entry.name)) continue
      let text
      try {
        text = await readFile(path, 'utf8')
      } catch {
        // Not UTF-8: a compiled artifact under a text extension, handled by the
        // read-back check rather than by rewriting.
        continue
      }
      let next = text
      for (const [from, to] of spellings) next = next.split(from).join(to)
      if (next === text) continue
      await writeFile(path, next)
      files += 1
    }
  }
  await walk(dir)
  if (files > 0) console.log(`pack-sidecar: scrubbed checkout paths in ${String(files)} file(s)`)
}

/**
 * Fail when any file under `dir` still contains the build user's home path.
 *
 * This is the shipped-artifact guarantee, so it reads bytes rather than text
 * and covers compiled executables, source maps, and data files alike. It runs
 * over the finished bundle, after every step that can introduce a path.
 * @param {string} dir - directory to read back.
 */
async function assertNoBuildMachinePaths(dir) {
  const home = Buffer.from(homedir())
  const leaked = []
  const walk = async (current) => {
    for (const entry of await readdir(current, { withFileTypes: true })) {
      const path = join(current, entry.name)
      if (entry.isSymbolicLink()) continue
      if (entry.isDirectory()) {
        await walk(path)
        continue
      }
      if (!entry.isFile()) continue
      if ((await readFile(path)).includes(home)) leaked.push(relative(dir, path))
    }
  }
  await walk(dir)
  if (leaked.length > 0) {
    throw new Error(`pack-sidecar: build-machine paths remain in ${String(leaked.length)} file(s): ${leaked.slice(0, 8).join(', ')}`)
  }
  console.log('pack-sidecar: no build-machine paths in the packaged tree')
}

/**
 * Remove symlinks with absolute targets. npm writes absolute .bin shims when
 * dependencies are file: URLs; Gatekeeper rejects absolute symlinks inside an
 * app bundle, and dsh web never invokes npm bins, so the shims are dead weight.
 * Relative links are module-resolution structure and must survive.
 */
async function stripAbsoluteSymlinks(dir) {
  let removed = 0
  const walk = async (current) => {
    for (const entry of await readdir(current, { withFileTypes: true })) {
      const path = join(current, entry.name)
      if (entry.isDirectory()) {
        await walk(path)
        continue
      }
      if (!entry.isSymbolicLink()) continue
      const target = await readlink(path)
      if (!target.startsWith('/')) continue
      await rm(path)
      removed += 1
    }
  }
  await walk(dir)
  if (removed > 0) console.log('pack-sidecar: stripped ' + removed + ' absolute symlink(s)')
}

/**
 * Rewrite absolute symlinks under `dir` into relative targets inside `dir`.
 * pnpm's deployed tree carries absolute links; Gatekeeper rejects absolute
 * symlinks inside an app bundle, so the embedded payload must be relativized
 * before signing. Non-absolute links and links pointing outside `dir` are
 * left untouched.
 */
async function relativizeSymlinks(dir) {
  const root = resolve(dir)
  let fixed = 0
  const walk = async (current) => {
    for (const entry of await readdir(current, { withFileTypes: true })) {
      const path = join(current, entry.name)
      if (entry.isDirectory()) {
        await walk(path)
        continue
      }
      if (!entry.isSymbolicLink()) continue
      const target = await readlink(path)
      if (!target.startsWith('/') || !target.startsWith(root) || !existsSync(target)) continue
      await rm(path)
      await symlink(relative(dirname(path), target), path)
      fixed += 1
    }
  }
  await walk(root)
  if (fixed > 0) console.log(`pack-sidecar: relativized ${fixed} symlink(s) under ${dir}`)
}

const step = process.argv[2] ?? 'all'
if (step === 'deploy' || step === 'all' || step === 'manifest') await deployApp()
if (step === 'runtime' || step === 'all') await installNodeRuntime()
if (step === 'check' || step === 'all') await selfCheck()
if (step === 'bundle') await bundleApp()
if (step === 'app-path') console.log(appBundlePath())
if (step === 'embed') await embedIntoApp()
console.log('pack-sidecar: ok')
