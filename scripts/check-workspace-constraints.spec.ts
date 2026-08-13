import { describe, expect, it } from 'vitest'

/** Keep in sync with `releaseMemberDirectory` in check-workspace-constraints.ts. */
const releaseMemberDirectory = /^(?:packages\/[^/]+\/[^/]+|apps\/(?:cli|web)|vendor\/[^/]+)$/

describe('releaseMemberDirectory', () => {
  it('keeps the publishable CLI and Web apps and omits the desktop host', () => {
    expect(releaseMemberDirectory.test('apps/cli')).toBe(true)
    expect(releaseMemberDirectory.test('apps/web')).toBe(true)
    expect(releaseMemberDirectory.test('apps/desktop')).toBe(false)
    expect(releaseMemberDirectory.test('packages/boot/app-boot')).toBe(true)
    expect(releaseMemberDirectory.test('vendor/cordis')).toBe(true)
  })
})
