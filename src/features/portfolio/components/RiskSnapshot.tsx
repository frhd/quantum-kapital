import { useEffect, useState } from "react"
import { listen } from "@tauri-apps/api/event"
import { AlertTriangle, ShieldCheck } from "lucide-react"

import { Card, CardContent, CardHeader, CardTitle } from "../../../shared/components/ui/card"
import {
  formatCents,
  formatPctNlv,
  portfolioRiskSnapshot,
  type PortfolioRisk,
  type PortfolioRiskChangedPayload,
} from "../../../shared/api/portfolioRisk"
import { cn } from "../../../shared/lib/utils"

/**
 * Phase 8 — top-of-portfolio header card. Shows the live total open
 * dollar-risk, "if all stops hit" P&L, NLV %, and a per-sector
 * exposure bar. Subscribes to `portfolio-risk-changed` so the bar
 * stays current without polling.
 */
export function RiskSnapshot() {
  const [risk, setRisk] = useState<PortfolioRisk | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)

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
    const unlistenPromise = listen<PortfolioRiskChangedPayload>("portfolio-risk-changed", () => {
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
          Loading portfolio risk…
        </CardContent>
      </Card>
    )
  }

  if (error || !risk) {
    return (
      <Card className="border-border/50 bg-card/30">
        <CardContent className="text-muted-foreground py-3 text-xs">
          Portfolio risk unavailable: {error ?? "no data"}
        </CardContent>
      </Card>
    )
  }

  const totalPct = risk.total_dollar_risk_cents / Math.max(1, risk.nlv_cents)
  const isHot = totalPct >= 0.08 // approaching default 10% limit
  const stopsEstimated = risk.open_positions.some((p) => p.stop_estimated)
  const sectorTotal = Math.max(
    1,
    risk.by_sector.reduce((acc, s) => acc + s.dollar_risk_cents, 0),
  )

  return (
    <Card className="border-border/50 bg-card/30">
      <CardHeader className="pb-2">
        <CardTitle className="text-foreground flex items-center justify-between text-sm font-medium">
          <span className="flex items-center gap-2">
            <ShieldCheck className="h-4 w-4 text-cyan-400/70" />
            Portfolio risk
          </span>
          <span className="text-muted-foreground text-[11px] font-normal">
            {risk.open_positions.length} open · NLV {formatCents(risk.nlv_cents)}
          </span>
        </CardTitle>
      </CardHeader>
      <CardContent className="space-y-3">
        <div className="flex items-baseline gap-3">
          <p className={cn("text-foreground text-2xl font-bold", isHot && "text-amber-300")}>
            {formatCents(risk.total_dollar_risk_cents)}
          </p>
          <p className="text-muted-foreground text-xs">
            ({formatPctNlv(risk.total_dollar_risk_cents, risk.nlv_cents)} of NLV · "if all stops
            hit")
          </p>
        </div>

        {stopsEstimated && (
          <div className="text-foreground/70 inline-flex items-center gap-1 text-[11px]">
            <AlertTriangle className="h-3 w-3 text-amber-400" />
            Some position stops were estimated (5% fallback)
          </div>
        )}

        <div className="space-y-1">
          <div className="text-muted-foreground text-[11px] tracking-wide uppercase">By sector</div>
          {risk.by_sector.length === 0 ? (
            <div className="text-foreground/50 text-xs">No open positions</div>
          ) : (
            <div className="border-border/40 flex h-3 overflow-hidden rounded-sm border">
              {risk.by_sector.map((s) => {
                const pct = (s.dollar_risk_cents / sectorTotal) * 100
                return (
                  <div
                    key={s.label}
                    title={`${s.label} — ${formatCents(s.dollar_risk_cents)} (${pct.toFixed(0)}%)`}
                    style={{ width: `${pct}%` }}
                    className={cn("h-full", sectorTint(s.label))}
                  />
                )
              })}
            </div>
          )}
          <div className="flex flex-wrap gap-x-3 gap-y-0.5">
            {risk.by_sector.map((s) => (
              <div
                key={s.label}
                className="text-foreground/70 inline-flex items-center gap-1 text-[10px]"
              >
                <span className={cn("h-2 w-2 rounded-sm", sectorTint(s.label))} />
                {s.label} · {formatCents(s.dollar_risk_cents)} · {s.position_count}
              </div>
            ))}
          </div>
        </div>
      </CardContent>
    </Card>
  )
}

/** Stable color per sector — ensures the bar segment color matches
 *  the legend dot. Falls back to slate for unknown sectors. */
function sectorTint(sector: string): string {
  switch (sector) {
    case "tech":
      return "bg-cyan-500/70"
    case "semis":
      return "bg-fuchsia-500/70"
    case "financials":
      return "bg-emerald-500/70"
    case "healthcare":
      return "bg-rose-500/70"
    case "energy":
      return "bg-amber-500/70"
    case "consumer_discretionary":
      return "bg-purple-500/70"
    case "consumer_staples":
      return "bg-orange-500/70"
    case "communications":
      return "bg-pink-500/70"
    case "industrials":
      return "bg-yellow-500/70"
    case "real_estate":
      return "bg-lime-500/70"
    case "broad_market":
      return "bg-sky-500/70"
    default:
      return "bg-slate-500/70"
  }
}
