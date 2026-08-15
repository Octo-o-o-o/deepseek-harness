/**
 * Package-owned invariant companion for `@deepseek-ai/dsh-client-ui-plugin-restart`.
 * @module @deepseek-ai/dsh-client-ui-plugin-restart/invariant
 */

/* jscpd:ignore-start */
import type { Context } from '@deepseek-ai/cordis'
import type { InvariantInstaller } from '@deepseek-ai/dsh-invariants'

const PACKAGE_NAME = '@deepseek-ai/dsh-client-ui-plugin-restart'

/** Cordis companion plugin name. */
export const name = 'client-ui-plugin-restart-invariant'
/** Service required before the companion can reserve package ownership. */
export const inject = ['invariants']

/**
 * No runtime invariant: the node half has no behavior, and the browser half
 * owns one slot registration plus one dictionary, both released by the same
 * effect disposer. The pending-restart state lives in the desktop shell, not
 * in this process, so there is no second authority here to compare against.
 */
const install: InvariantInstaller = () => {}

/**
 * Register this package's invariant companion.
 * @param ctx - Cordis context carrying the invariant service.
 * @returns the installed registration's disposer after setup succeeds.
 */
export const apply = (ctx: Context): Promise<() => void> =>
  Promise.resolve(ctx.invariants.register(PACKAGE_NAME, install))
/* jscpd:ignore-end */
