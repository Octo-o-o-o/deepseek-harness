/**
 * Desktop `--patch` overlay: the packaged composition turns off the shared
 * official brand row (those slots are `single`) and mounts the restart plugin.
 */
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

const patch = readFileSync(
  join(dirname(fileURLToPath(import.meta.url)), '../desktop.patch.yml'),
  'utf8',
)

describe('desktop.patch.yml', () => {
  it('disables the shared official brand occupants', () => {
    expect(patch).toMatch(/^- id: ui-brand-official\n {2}disabled: true$/m)
  })

  it('inserts the desktop-only restart plugin', () => {
    expect(patch).toContain('- id: ui-plugin-restart')
    expect(patch).toContain("name: '@deepseek-ai/dsh-client-ui-plugin-restart'")
  })
})
