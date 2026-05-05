/**
 * Phase 4 (quant-decisions): per-strategy attribution table.
 *
 * One row per detector class (`breakout`, `parabolic_short`,
 * `episodic_pivot`, …) plus an `unattributed` bucket for legs whose
 * opening fill carried no strategy linkage. Pulled via
 * `tradeReviewMetricsApi.getStrategyRollup` over a date range.
 *
 * `null` cells render as `—` (insufficient data — e.g. missing
 * setup-id linkage means `avg_r` cannot be reduced). Profit-factor
 * with no losses comes through as `Number.POSITIVE_INFINITY` and
 * renders as `∞`.
 */

import type { StrategyRollup as StrategyRollupRow } from "../types"

function fmtR(v: number | null): string {
  if (v == null) return "—"
  const sign = v >= 0 ? "+" : ""
  return `${sign}${v.toFixed(2)}R`
}

function fmtPct(v: number | null): string {
  if (v == null) return "—"
  return `${(v * 100).toFixed(1)}%`
}

function fmtNum(v: number | null): string {
  if (v == null) return "—"
  if (!Number.isFinite(v)) return v === Number.POSITIVE_INFINITY ? "∞" : "—"
  return v.toFixed(2)
}

function fmtPnl(v: number): string {
  const sign = v >= 0 ? "+" : ""
  return `${sign}$${v.toFixed(2)}`
}

export interface StrategyRollupProps {
  rows: StrategyRollupRow[]
}

export function StrategyRollup({ rows }: StrategyRollupProps) {
  if (rows.length === 0) {
    return (
      <div className="text-muted-foreground text-sm" role="status">
        No strategy attribution for this range yet.
      </div>
    )
  }
  return (
    <div className="border-border overflow-hidden rounded border">
      <table className="w-full text-sm" aria-label="Per-strategy attribution">
        <thead className="bg-secondary/40 text-muted-foreground text-xs uppercase">
          <tr>
            <th className="px-2 py-1 text-left">Strategy</th>
            <th className="px-2 py-1 text-right">N</th>
            <th className="px-2 py-1 text-right">PnL</th>
            <th className="px-2 py-1 text-right">Avg R</th>
            <th className="px-2 py-1 text-right">Win</th>
            <th className="px-2 py-1 text-right">PF</th>
            <th className="px-2 py-1 text-right">Sharpe</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((r) => (
            <tr key={r.strategy} className="odd:bg-secondary/10">
              <td className="px-2 py-1 font-mono text-xs">{r.strategy}</td>
              <td className="px-2 py-1 text-right font-mono">{r.n_trades}</td>
              <td
                className={`px-2 py-1 text-right font-mono ${r.realized_pnl >= 0 ? "text-green-500" : "text-red-500"}`}
              >
                {fmtPnl(r.realized_pnl)}
              </td>
              <td className="px-2 py-1 text-right font-mono">{fmtR(r.avg_r)}</td>
              <td className="px-2 py-1 text-right font-mono">{fmtPct(r.win_rate)}</td>
              <td className="px-2 py-1 text-right font-mono">{fmtNum(r.profit_factor)}</td>
              <td className="px-2 py-1 text-right font-mono">{fmtNum(r.sharpe_30d)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}
