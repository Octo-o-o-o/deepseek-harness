// Desktop application mark: the packaged app icon drawn as vector — the
// whale glyph on the icon's dark plate. Its colors are literal rather than
// `--dsw-*` tokens because this mark identifies the INSTALLED APPLICATION and
// must stay recognizable as the Dock/taskbar icon in either theme; the
// hairline ring keeps the plate's edge visible on a dark surface.

import type { IconProps } from './icons/props.ts'
import { WHALE_PATH, WHALE_VIEWBOX } from './whale-path.ts'

/** Mark canvas edge; every literal below is expressed against it. */
const CANVAS = 32

/** Plate corner radius: the packaged icon's 22.37% of the edge. */
const PLATE_RADIUS = 7.16

/** Whale width on the canvas (62.5% of the edge, as in the packaged icon). */
const WHALE_WIDTH = 20

const scale = WHALE_WIDTH / WHALE_VIEWBOX.width
const offsetX = (CANVAS - WHALE_WIDTH) / 2
const offsetY = (CANVAS - WHALE_VIEWBOX.height * scale) / 2

/**
 * Render the desktop application mark.
 * @param props.size - square edge in px (default 32, the mark's drawn size).
 * @param props.className - extra class for layout placement.
 * @returns the mark svg (aria-hidden decorative brand art).
 */
export function AppMark({ size = CANVAS, className }: IconProps) {
  return (
    <svg
      width={size}
      height={size}
      className={className}
      viewBox={`0 0 ${CANVAS} ${CANVAS}`}
      fill="none"
      aria-hidden="true"
    >
      <rect width={CANVAS} height={CANVAS} rx={PLATE_RADIUS} fill="#0B0B0C"/>
      <rect
        x="0.5" y="0.5" width={CANVAS - 1} height={CANVAS - 1} rx={PLATE_RADIUS - 0.5}
        stroke="#FFFFFF" strokeOpacity="0.14"
      />
      <g transform={`translate(${offsetX} ${offsetY.toFixed(2)}) scale(${scale.toFixed(5)})`}>
        <path d={WHALE_PATH} fill="#FFFFFF"/>
      </g>
    </svg>
  )
}
