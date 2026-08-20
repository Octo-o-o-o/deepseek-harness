/**
 * Desktop-only browser plugin: the sidebar-foot restart action, and the
 * packaged application's occupants for the generic brand slots.
 *
 * `dsh plugin add` rewrites the profile manifest, but the composition took its
 * bundle list once at boot — only the patch files are re-read live — so a
 * newly installed plugin stays dark until the application starts again. The
 * shell owns both halves of that fact: it stamped the manifest when it
 * launched this sidecar, and only it can replace the process. This half asks
 * and confirms; it decides nothing.
 *
 * The brand occupants fill the generic `sidebar.brand.*` and
 * `conversation.hero.brand.mark` holes. They ride this package because the
 * desktop patch layer is the only composition that mounts it.
 * @module @deepseek-ai/dsh-client-ui-plugin-restart/client
 */

import type { ClientContext } from '@deepseek-ai/dsh-client-runtime/client'
// Type-only: pulls the locale plugin's Context merge (ctx.locale).
import type {} from '@deepseek-ai/dsh-client-locale/client'
// Type-only: pulls the sidebar's SlotMap merge (footer.action + brand holes).
import type {} from '@deepseek-ai/dsh-client-ui-sidebar/client'
// Type-only: pulls the conversation Hero brand-mark hole.
import type {} from '@deepseek-ai/dsh-client-ui-conversation/client'
import { DesktopBrandMark, DesktopBrandName } from './Brand.tsx'
import { PluginRestartAction } from './PluginRestartAction.tsx'
import { en, zh } from './locales.ts'

export type { PluginRestartKey } from './locales.ts'
export type { PluginRestartActionProps } from './slots.ts'

/** Dictionary namespace owned by this plugin. */
const NS = 'pluginRestart'

/** Required services: the slot registry and the copy. */
export const inject = ['slots', 'locale']

/**
 * Client plugin body: one sidebar-foot entry, hidden until the shell reports
 * that a restart would change the composition, plus the desktop brand
 * occupants for the generic brand slots.
 * @param ctx - client root context.
 */
export function apply(ctx: ClientContext): void {
  ctx.effect(() => ctx.locale.register(NS, { zh, en }), 'ui-plugin-restart: dictionaries')

  ctx.slots.inject('sidebar.footer.action', () => ctx.slots.register({
    name: 'sidebar.footer.action',
    id: 'plugin-restart',
    // After the shipped entries: this one is absent on most launches, and a
    // control that appears at the top would push the settled ones around.
    order: 200,
    locale: NS,
  }, PluginRestartAction))

  ctx.slots.inject('sidebar.brand.mark', () =>
    ctx.slots.inject('sidebar.brand.name', () =>
      ctx.slots.inject('conversation.hero.brand.mark', function* () {
        yield ctx.slots.register({ name: 'sidebar.brand.mark' }, DesktopBrandMark)
        yield ctx.slots.register({ name: 'sidebar.brand.name' }, DesktopBrandName)
        yield ctx.slots.register({ name: 'conversation.hero.brand.mark' }, DesktopBrandMark)
      })))
}
