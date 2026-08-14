// DeepSeek fish logo. Native 23.16x17.04, rendered 24x18 by default; hero
// usage scales to 34x25. Color rides currentColor (wordmark ink).

import type { IconProps } from './icons/props.ts'
import { WHALE_PATH, WHALE_VIEWBOX } from './whale-path.ts'

/**
 * Render the fish logo.
 * @param props.size - width in px (default 24; height keeps the 23.16:17.04 ratio).
 * @param props.className - extra class for layout placement.
 * @returns the logo svg (aria-hidden; pair with the wordmark for accessibility).
 */
export function FishLogo({ size = 24, className }: IconProps) {
  return (
    <svg
      width={size}
      height={(size * WHALE_VIEWBOX.height) / WHALE_VIEWBOX.width}
      className={className}
      viewBox={`0 0 ${WHALE_VIEWBOX.width} ${WHALE_VIEWBOX.height}`}
      fill="none"
      aria-hidden="true"
    >
      <path d={WHALE_PATH} fill="currentColor"/>
    </svg>
  )
}
