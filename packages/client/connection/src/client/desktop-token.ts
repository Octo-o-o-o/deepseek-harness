/** Header the browser client attaches when `window.__DSH_TOKEN__` is present. */
export const DESKTOP_TOKEN_HEADER = 'X-DSH-Token'

/**
 * Read the page-injected per-launch token.
 *
 * @returns the token, or undefined when the page did not inject one.
 */
export function readPageDesktopToken(): string | undefined {
  const root = globalThis as { __DSH_TOKEN__?: unknown; window?: { __DSH_TOKEN__?: unknown } }
  const token = root.__DSH_TOKEN__ ?? root.window?.__DSH_TOKEN__
  return typeof token === 'string' && token !== '' ? token : undefined
}

/**
 * Copy `init` and add `X-DSH-Token` when a page token exists.
 *
 * @param init - original fetch init; omitted fields stay omitted when no token.
 * @returns init with the token header, or the original object when none is set.
 */
export function withDesktopToken(init?: RequestInit): RequestInit {
  const token = readPageDesktopToken()
  if (token === undefined) return init ?? {}
  const headers = new Headers(init?.headers)
  headers.set(DESKTOP_TOKEN_HEADER, token)
  return { ...init, headers }
}
