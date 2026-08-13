#!/usr/bin/env node
/**
 * Build the desktop sidecar: production `pnpm deploy` of @deepseek-ai/dsh,
 * a pinned Node runtime, and a PATH-stripped boot self-check.
 *
 * Zero npm dependencies. Run from the repository root or apps/desktop.
 */

import { createHash } from 'node:crypto'
import { createWriteStream } from 'node:fs'
import { mkdir, readFile, rm, chmod, cp, readdir, lstat, symlink } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { pipeline } from 'node:stream/promises'
import { fileURLToPath } from 'node:url'
import { spawn } from 'node:child_process'

const NODE_VERSION = '24.19.0'
const NODE_ARCHIVES = {
  'darwin-arm64': `node-v${NODE_VERSION}-darwin-arm64.tar.gz`,
  'darwin-x64': `node-v${NODE_VERSION}-darwin-x64.tar.gz`,
  'win32-x64': `node-v${NODE_VERSION}-win-x64.zip`,
}

const here = dirname(fileURLToPath(import.meta.url))
const desktopRoot = resolve(here, '..')
const repoRoot = resolve(desktopRoot, '../..')
const sidecarRoot = join(desktopRoot, 'sidecar')
const cacheDir = join(sidecarRoot, 'cache')
const distDir = join(sidecarRoot, 'dist')
const appDir = join(distDir, 'app')
const binDir = join(distDir, 'bin')

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

async function deployApp() {
  await rm(appDir, { recursive: true, force: true })
  await mkdir(distDir, { recursive: true })
  await run('pnpm', ['--filter', '@deepseek-ai/dsh', 'deploy', '--prod', '--legacy', appDir])
  const required = [
    join(appDir, 'lib/bin.js'),
    join(appDir, 'node_modules/@deepseek-ai/dsh-web-app/package.json'),
    join(appDir, 'node_modules/@deepseek-ai/dsh-base/package.json'),
  ]
  for (const path of required) {
    try {
      await readFile(path)
    } catch {
      throw new Error(`deploy missing ${path}`)
    }
  }
  const frontend = await capture(process.execPath, ['-e', `
    const { createRequire } = require('node:module')
    const { dirname } = require('node:path')
    const webApp = dirname(require.resolve('@deepseek-ai/dsh-web-app/package.json'))
    console.log(createRequire(webApp + '/package.json').resolve('@deepseek-ai/dsh-web-frontend/dist/index.html'))
  `], { cwd: appDir })
  if (frontend.code !== 0 || !frontend.stdout.includes('index.html')) {
    throw new Error(`deploy cannot resolve frontend dist: ${frontend.stderr}`)
  }
  await hoistPnpmStore(appDir)
}

/**
 * Symlink every package in the isolated `.pnpm` store into `node_modules`
 * so ESM parent-walk from a realpathed package can see peer Service
 * Definitions that `pnpm deploy --prod` did not hoist.
 * @param {string} deployedApp
 */
async function hoistPnpmStore(deployedApp) {
  const pnpmDir = join(deployedApp, 'node_modules/.pnpm')
  const rootNm = join(deployedApp, 'node_modules')
  let entries
  try {
    entries = await readdir(pnpmDir, { withFileTypes: true })
  } catch {
    throw new Error(`deploy missing isolated store ${pnpmDir}`)
  }
  for (const entry of entries) {
    if (!entry.isDirectory()) continue
    await hoistNodeModules(join(pnpmDir, entry.name, 'node_modules'), rootNm)
  }
}

/**
 * @param {string} fromNm
 * @param {string} rootNm
 */
async function hoistNodeModules(fromNm, rootNm) {
  let names
  try {
    names = await readdir(fromNm, { withFileTypes: true })
  } catch {
    return
  }
  for (const item of names) {
    if (item.name.startsWith('.')) continue
    const src = join(fromNm, item.name)
    if (item.name.startsWith('@')) {
      await hoistNodeModules(src, join(rootNm, item.name))
      continue
    }
    const dest = join(rootNm, item.name)
    if (await pathExists(dest)) continue
    await mkdir(dirname(dest), { recursive: true })
    try {
      await symlink(src, dest)
    } catch (error) {
      if (error && typeof error === 'object' && 'code' in error && error.code === 'EEXIST') continue
      throw error
    }
  }
}

/**
 * @param {string} path
 * @returns {Promise<boolean>}
 */
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
  const sumsUrl = `https://nodejs.org/dist/v${NODE_VERSION}/SHASUMS256.txt`
  await mkdir(cacheDir, { recursive: true })
  const archivePath = join(cacheDir, archive)
  const sumsPath = join(cacheDir, `SHASUMS256-${NODE_VERSION}.txt`)
  if (!(await exists(archivePath))) await download(url, archivePath)
  if (!(await exists(sumsPath))) await download(sumsUrl, sumsPath)
  const expected = (await readFile(sumsPath, 'utf8'))
    .split('\n')
    .map(line => line.trim())
    .find(line => line.endsWith(`  ${archive}`))
    ?.slice(0, 64)
  if (expected === undefined) throw new Error(`no sha256 for ${archive}`)
  const actual = await sha256File(archivePath)
  if (actual !== expected) throw new Error(`sha256 mismatch for ${archive}`)
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

async function selfCheck() {
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
 * Tauri's resource copy drops directory symlinks, so the bundled
 * `node_modules` would keep `.pnpm` and lose every hoisted package.
 * Re-copy with `cp -a` after `tauri build` so the .app matches the
 * self-checked tree.
 */
async function embedIntoApp() {
  const appPath = join(
    desktopRoot,
    'src-tauri/target/release/bundle/macos/dshd.app',
  )
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
  const node = join(destBin, 'node')
  const binJs = join(destApp, 'lib/bin.js')
  const boot = join(destApp, 'node_modules/@deepseek-ai/dsh-app-boot/package.json')
  for (const path of [node, binJs, boot]) {
    if (!(await pathExists(path))) throw new Error(`embed missing ${path}`)
  }
}

const step = process.argv[2] ?? 'all'
if (step === 'deploy' || step === 'all') await deployApp()
if (step === 'runtime' || step === 'all') await installNodeRuntime()
if (step === 'check' || step === 'all') await selfCheck()
if (step === 'embed') await embedIntoApp()
console.log('pack-sidecar: ok')
