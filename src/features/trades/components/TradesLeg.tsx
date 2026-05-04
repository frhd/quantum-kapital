/**
 * Phase 3 — single-leg row inside a `TradesGroup`.
 *
 * Renders one IBKR fill: ET time, side, qty, avg price, commission,
 * realized P&L. Missing commission renders as `—` (em dash) — never
 * coalesced to `0`, which would silently mis-attribute fees.
 *
 * Time conversion: the row carries UTC; this component is the only
 * place we cross into ET for display so a stray `new Date(time)` in a
 * caller can't drift into the host TZ.
 */

import type { ExecutionRow } from "../types"

const ET_TIME_FMT = new Intl.DateTimeFormat("en-US", {
  timeZone: "America/New_York",
  hour: "2-digit",
  minute: "2-digit",
  second: "2-digit",
  hour12: false,
})

function formatEtTime(iso: string): string {
  const d = new Date(iso)
  if (Number.isNaN(d.getTime())) return "—"
  return ET_TIME_FMT.format(d)
}

function fmtUsd(value: number): string {
  const sign = value < 0 ? "-" : ""
  const abs = Math.abs(value)
  return `${sign}$${abs.toFixed(2)}`
}

export function TradesLeg({ leg }: { leg: ExecutionRow }) {
  const isUsd = !leg.currency || leg.currency === "USD"
  const realizedClass =
    leg.realized_pnl === undefined
      ? "text-muted-foreground"
      : leg.realized_pnl >= 0
        ? "text-green-400"
        : "text-red-400"
  return (
    <tr
      className="border-border/40 hover:bg-muted/20 border-t font-mono text-xs tabular-nums"
      data-testid="trades-leg"
    >
      <td className="py-1.5 pr-3 pl-3 whitespace-nowrap">{formatEtTime(leg.time)}</td>
      <td className="py-1.5 pr-3 capitalize">
        <span
          className={leg.side === "bought" ? "text-blue-400" : "text-amber-400"}
          data-testid="trades-leg-side"
        >
          {leg.side}
        </span>
      </td>
      <td className="py-1.5 pr-3 text-right">{leg.qty}</td>
      <td className="py-1.5 pr-3 text-right">{fmtUsd(leg.avg_price)}</td>
      <td className="py-1.5 pr-3 text-right" data-testid="trades-leg-commission">
        {leg.commission === undefined ? "—" : fmtUsd(leg.commission)}
      </td>
      <td className={`py-1.5 pr-3 text-right ${realizedClass}`} data-testid="trades-leg-realized">
        {leg.realized_pnl === undefined ? "—" : fmtUsd(leg.realized_pnl)}
      </td>
      <td className="py-1.5 pr-3 text-right">
        {!isUsd && leg.currency ? (
          <span className="text-muted-foreground">{leg.currency}</span>
        ) : null}
      </td>
    </tr>
  )
}
