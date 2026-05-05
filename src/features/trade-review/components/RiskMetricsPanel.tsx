/**
 * Phase 4 (quant-decisions): risk-metrics card grid.
 *
 * Renders Sharpe / Sortino / Calmar / profit-factor / expectancy /
 * max-DD / win-rate / avg win-R / avg loss-R as a six-cell grid. Each
 * cell renders `—` when the metric is `null` (Phase 4 uses `null` as
 * the "insufficient history" sentinel — N < 20 daily samples for
 * Sharpe/Sortino/Calmar; no losses for profit-factor when wins=0).
 *
 * Profit factor with no losses but positive wins comes through as
 * `Number.POSITIVE_INFINITY` from the Rust side; we render it as `∞`.
 */

import type { RiskMetrics } from "../types"

interface Cell {
  label: string
  value: string
  hint?: string
}

function fmt(v: number | null | undefined, digits = 2): string {
  if (v == null || !Number.isFinite(v)) return v === Number.POSITIVE_INFINITY ? "∞" : "—"
  return v.toFixed(digits)
}

function fmtPct(v: number | null | undefined): string {
  if (v == null) return "—"
  return `${(v * 100).toFixed(1)}%`
}

function fmtR(v: number | null | undefined): string {
  if (v == null) return "—"
  const sign = v >= 0 ? "+" : ""
  return `${sign}${v.toFixed(2)}R`
}

export interface RiskMetricsPanelProps {
  metrics: RiskMetrics | null | undefined
}

export function RiskMetricsPanel({ metrics }: RiskMetricsPanelProps) {
  if (!metrics) {
    return (
      <div className="text-muted-foreground text-sm" role="status">
        No risk metrics for this range yet.
      </div>
    )
  }
  const cells: Cell[] = [
    {
      label: "Sharpe",
      value: fmt(metrics.sharpe),
      hint: metrics.sharpe == null ? "N < 20 days" : "annualized",
    },
    {
      label: "Sortino",
      value: fmt(metrics.sortino),
      hint: metrics.sortino == null ? "N < 20 days" : "annualized",
    },
    {
      label: "Calmar",
      value: fmt(metrics.calmar),
      hint: metrics.max_dd === 0 ? "no DD" : "annualized",
    },
    { label: "Profit factor", value: fmt(metrics.profit_factor) },
    { label: "Expectancy", value: fmtR(metrics.expectancy_r) },
    {
      label: "Max DD",
      value: fmtPct(metrics.max_dd),
      hint: `${metrics.max_dd_duration} day(s)`,
    },
    { label: "Win rate", value: fmtPct(metrics.win_rate) },
    { label: "Avg win", value: fmtR(metrics.avg_win_r) },
    { label: "Avg loss", value: fmtR(metrics.avg_loss_r) },
  ]
  return (
    <div className="space-y-2">
      <div className="text-muted-foreground flex justify-between text-xs">
        <span>
          {metrics.n_days} day{metrics.n_days === 1 ? "" : "s"} · {metrics.n_trades} trade
          {metrics.n_trades === 1 ? "" : "s"}
        </span>
        <span>rf {(metrics.risk_free_rate_annual * 100).toFixed(2)}%</span>
      </div>
      <div className="grid grid-cols-3 gap-2">
        {cells.map((c) => (
          <div key={c.label} className="border-border bg-secondary/40 rounded border p-2">
            <div className="text-muted-foreground text-[10px] tracking-wide uppercase">
              {c.label}
            </div>
            <div className="font-mono text-sm font-semibold">{c.value}</div>
            {c.hint && <div className="text-muted-foreground text-[10px]">{c.hint}</div>}
          </div>
        ))}
      </div>
    </div>
  )
}
