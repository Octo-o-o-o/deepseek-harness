/**
 * Packaged-application brand occupants for the generic sidebar and Hero slots.
 * Mounted only through the desktop patch layer, so `dsh web` in a browser
 * keeps the shell fallback / official occupants.
 * @module @deepseek-ai/dsh-client-ui-plugin-restart/client/Brand
 */

import { AppMark } from '@deepseek-ai/dsh-client-ui-primitives'
import type { HeroBrandMarkOwnerProps } from '@deepseek-ai/dsh-client-ui-conversation/client'
import type { SidebarBrandMarkOwnerProps } from '@deepseek-ai/dsh-client-ui-sidebar/client'
import css from './Brand.module.css'

type DesktopBrandMarkProps = HeroBrandMarkOwnerProps & SidebarBrandMarkOwnerProps

/**
 * Product name in the desktop shell's brand row, split across two lines so
 * the full name fits the column at its default width. Brand text is the same
 * in every locale, so it stays out of the dictionaries.
 */
const DESKTOP_BRAND = { name: 'DeepSeek Harness', edition: 'Desktop' } as const

/**
 * Render the packaged application mark at the size the host surface requests.
 * @param props - Host-supplied mark presentation.
 * @returns the desktop application mark.
 */
export function DesktopBrandMark({ size, className }: DesktopBrandMarkProps) {
  return <AppMark size={size} className={className} />
}

/**
 * Render the two-line packaged application name beside the expanded mark.
 * @returns the desktop product name.
 */
export function DesktopBrandName() {
  return (
    <span className={css.appName}>
      <span className={css.appNameLine}>{DESKTOP_BRAND.name}</span>
      <span className={css.appNameEdition}>{DESKTOP_BRAND.edition}</span>
    </span>
  )
}
