/**
 * Phase 4 (quant-decisions): daily equity-curve sparkline with
 * drawdown shading.
 *
 * Renders a tiny SVG line chart. Each point's x-position is its date
 * index (uniform spacing — the curve only carries dates that had any
 * activity). DD shading is drawn under the curve where equity is
 * below the running peak. Avoids d3/recharts to keep the bundle
 * lean — Phase 4 only needs a glanceable visualization.
 */

import { useMemo } from "react"

import type { EquityPoint } from "../types"

const PAD_X = 4
const PAD_Y = 6
const W = 320
const H = 80

export interface EquityCurveProps {
  points: EquityPoint[]
  /** Optional caption rendered below the chart. */
  caption?: string
}

export function EquityCurve({ points, caption }: EquityCurveProps) {
  const computed = useMemo(() => {
    if (points.length === 0) return null
    const min = Math.min(...points.map((p) => p.equity))
    const max = Math.max(...points.map((p) => p.equity))
    const range = max - min || 1
    const step = points.length > 1 ? (W - 2 * PAD_X) / (points.length - 1) : 0
    const xy = points.map((p, i) => {
      const x = PAD_X + i * step
      const y = PAD_Y + (H - 2 * PAD_Y) * (1 - (p.equity - min) / range)
      return { x, y, equity: p.equity, daily_pnl: p.daily_pnl, date: p.date }
    })
    // Drawdown shading: walk peaks, compute peak-line for each point.
    let peak = xy[0]?.equity ?? 0
    const peakLine = xy.map((pt) => {
      peak = Math.max(peak, pt.equity)
      const peakY = PAD_Y + (H - 2 * PAD_Y) * (1 - (peak - min) / range)
      return { x: pt.x, y: peakY }
    })
    return { xy, peakLine, min, max }
  }, [points])

  if (!computed) {
    return (
      <div className="text-muted-foreground text-sm" role="status">
        No equity curve for this range yet.
      </div>
    )
  }

  const linePath = computed.xy.map((p, i) => `${i === 0 ? "M" : "L"} ${p.x} ${p.y}`).join(" ")
  // Drawdown polygon: connect peakLine across the top and curve back.
  const ddPath =
    computed.peakLine.map((p, i) => `${i === 0 ? "M" : "L"} ${p.x} ${p.y}`).join(" ") +
    " " +
    computed.xy
      .slice()
      .reverse()
      .map((p) => `L ${p.x} ${p.y}`)
      .join(" ") +
    " Z"

  const last = computed.xy[computed.xy.length - 1]
  const first = computed.xy[0]
  const totalChange = last.equity - first.equity

  return (
    <div className="space-y-1">
      <div className="text-muted-foreground flex justify-between text-xs">
        <span>{points[0]?.date}</span>
        <span className={totalChange >= 0 ? "text-green-500" : "text-red-500"}>
          {totalChange >= 0 ? "+" : ""}
          {totalChange.toFixed(2)}
        </span>
        <span>{points[points.length - 1]?.date}</span>
      </div>
      <svg
        viewBox={`0 0 ${W} ${H}`}
        className="bg-secondary/40 border-border rounded border"
        role="img"
        aria-label="Equity curve"
      >
        <path d={ddPath} fill="rgba(248, 113, 113, 0.15)" />
        <path d={linePath} fill="none" stroke="currentColor" strokeWidth={1.5} />
      </svg>
      {caption && <div className="text-muted-foreground text-xs">{caption}</div>}
    </div>
  )
}
