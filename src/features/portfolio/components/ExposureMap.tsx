import { useEffect, useState } from "react"
import { listen } from "@tauri-apps/api/event"

import { Card, CardContent, CardHeader, CardTitle } from "../../../shared/components/ui/card"
import {
  formatCents,
  portfolioRiskSnapshot,
  type PortfolioRisk,
} from "../../../shared/api/portfolioRisk"
import { cn } from "../../../shared/lib/utils"

const FACTOR_AXES = ["momentum", "size", "value"] as const
const LEVELS = ["high", "mid", "low"] as const

type FactorAxis = (typeof FACTOR_AXES)[number]
type FactorLevel = (typeof LEVELS)[number]

/**
 * Phase 8 — sector × factor heatmap. Shows position counts per
 * (sector, factor-bucket) cell so the trader can see "I'm 3 deep in
 * high-momentum semis" before adding the 4th. Coarse: only the
 * three factors P8 ships (momentum, size, value); a future phase
 * can extend the axis set without changing the cell shape.
 */
export function ExposureMap() {
  const [risk, setRisk] = useState<PortfolioRisk | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let active = true
    portfolioRiskSnapshot()
      .then((r) => {
        if (active) {
          setRisk(r)
          setError(null)
        }
      })
      .catch((e) => active && setError(String(e)))
      .finally(() => active && setLoading(false))
    return () => {
      active = false
    }
  }, [])

  useEffect(() => {
    const unlistenPromise = listen("portfolio-risk-changed", () => {
      portfolioRiskSnapshot()
        .then(setRisk)
        .catch((e) => setError(String(e)))
    })
    return () => {
      void unlistenPromise.then((u) => u())
    }
  }, [])

  if (loading) {
    return (
      <Card className="border-border/50 bg-card/30">
        <CardContent className="text-muted-foreground py-3 text-xs">
          Loading exposure map…
        </CardContent>
      </Card>
    )
  }

  if (error || !risk) {
    return (
      <Card className="border-border/50 bg-card/30">
        <CardContent className="text-muted-foreground py-3 text-xs">
          Exposure map unavailable: {error ?? "no data"}
        </CardContent>
      </Card>
    )
  }

  const sectors = risk.by_sector.map((s) => s.label)
  const cellMatrix = buildCellMatrix(risk, sectors)
  const maxCount = Math.max(1, ...cellMatrix.flatMap((row) => row.map((c) => c.count)))

  return (
    <Card className="border-border/50 bg-card/30">
      <CardHeader className="pb-2">
        <CardTitle className="text-foreground text-sm font-medium">
          Sector × factor exposure
        </CardTitle>
      </CardHeader>
      <CardContent className="space-y-2">
        {sectors.length === 0 ? (
          <div className="text-foreground/50 text-xs">No open positions</div>
        ) : (
          <div className="overflow-x-auto">
            <table className="w-full border-collapse text-[11px]">
              <thead>
                <tr className="text-muted-foreground">
                  <th className="px-1 py-1 text-left font-normal">Sector</th>
                  {FACTOR_AXES.flatMap((axis) =>
                    LEVELS.map((lvl) => (
                      <th key={`${axis}_${lvl}`} className="px-1 py-1 text-center font-normal">
                        {axis === "size" ? sizeLabel(lvl) : `${axis}-${lvl}`}
                      </th>
                    )),
                  )}
                </tr>
              </thead>
              <tbody>
                {sectors.map((sector, rowIdx) => (
                  <tr key={sector} className="border-border/40 border-t">
                    <td className="text-foreground/80 px-1 py-1 font-mono">{sector}</td>
                    {cellMatrix[rowIdx].map((cell, idx) => {
                      const intensity = cell.count / maxCount
                      return (
                        <td
                          key={`${sector}-${idx}`}
                          className="px-1 py-1 text-center"
                          title={`${cell.label}: ${cell.count} position${cell.count === 1 ? "" : "s"} · ${formatCents(cell.dollarRisk)}`}
                        >
                          <span
                            className={cn(
                              "inline-block min-w-[24px] rounded-sm px-1 py-0.5 font-mono",
                              cell.count === 0
                                ? "text-foreground/30 bg-transparent"
                                : intensityClass(intensity),
                            )}
                          >
                            {cell.count}
                          </span>
                        </td>
                      )
                    })}
                  </tr>
                ))}
              </tbody>
            </table>
            <div className="text-muted-foreground mt-1 text-[10px]">
              Cells show position count; hover for dollar-risk. Heatmap is coarsely bucketed — 3
              deep in any cell is a flag.
            </div>
          </div>
        )}
      </CardContent>
    </Card>
  )
}

interface Cell {
  axis: FactorAxis
  level: FactorLevel | "small" | "mid" | "large" | "mega"
  label: string
  count: number
  dollarRisk: number
}

function buildCellMatrix(risk: PortfolioRisk, sectors: string[]): Cell[][] {
  return sectors.map((sector) => {
    const positions = risk.open_positions.filter((p) => p.sector === sector)
    return FACTOR_AXES.flatMap<Cell>((axis) =>
      LEVELS.map<Cell>((lvl) => {
        const targetLabel = axis === "size" ? sizeBucketLabel(lvl) : `${axis}_${lvl}`
        const matched = positions.filter((p) => {
          if (axis === "momentum") return p.factors.momentum === targetLabel
          if (axis === "value") return p.factors.value === targetLabel
          if (axis === "size") return p.factors.size === targetLabel
          return false
        })
        return {
          axis,
          level: lvl,
          label: targetLabel,
          count: matched.length,
          dollarRisk: matched.reduce((acc, p) => acc + p.dollar_risk_cents, 0),
        }
      }),
    )
  })
}

function sizeBucketLabel(level: FactorLevel): string {
  // Match `factors.rs::bucket_market_cap` outputs.
  switch (level) {
    case "high":
      return "size_mega"
    case "mid":
      return "size_large"
    case "low":
      return "size_mid"
  }
}

function sizeLabel(level: FactorLevel): string {
  switch (level) {
    case "high":
      return "size-mega"
    case "mid":
      return "size-large"
    case "low":
      return "size-mid"
  }
}

function intensityClass(intensity: number): string {
  if (intensity >= 0.75) return "bg-rose-500/40 text-rose-100"
  if (intensity >= 0.5) return "bg-amber-500/30 text-amber-100"
  if (intensity >= 0.25) return "bg-cyan-500/20 text-cyan-100"
  return "bg-cyan-500/10 text-cyan-100"
}
