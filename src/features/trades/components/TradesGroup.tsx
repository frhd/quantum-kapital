/**
 * Phase 3 — collapsible per-symbol (and per-option-key) group block.
 *
 * Header summarises the group: label, leg count, gross realized P&L,
 * fees, and net = gross - fees. A small "fees pending" badge surfaces
 * when at least one leg's commission has not yet been reported by IBKR
 * — the FE never silently substitutes `0`.
 *
 * Body renders the legs in execution order via `TradesLeg` once the
 * native `<details>` is open. Defaults to collapsed when there are
 * many legs, expanded for short groups (≤ 4) so most days read
 * end-to-end without a click.
 */

import { Badge } from "../../../shared/components/ui/badge"
import { TradesLeg } from "./TradesLeg"
import type { TradeGroup } from "../types"

function fmtUsd(value: number): string {
  const sign = value < 0 ? "-" : ""
  const abs = Math.abs(value)
  return `${sign}$${abs.toFixed(2)}`
}

function pnlClass(value: number): string {
  if (value > 0) return "text-green-400"
  if (value < 0) return "text-red-400"
  return "text-muted-foreground"
}

function groupLabel(group: TradeGroup): string {
  if (group.optionKey === null) return group.symbol
  const { expiry, strike, right } = group.optionKey
  return `${group.symbol} ${expiry} $${strike} ${right}`
}

export function TradesGroup({ group }: { group: TradeGroup }) {
  const label = groupLabel(group)
  const legCountLabel = group.legs.length === 1 ? "1 leg" : `${group.legs.length} legs`

  return (
    <details
      className="border-border bg-background/40 group rounded-md border"
      data-testid="trades-group"
      open={group.legs.length <= 4}
    >
      <summary
        className="hover:bg-muted/20 flex cursor-pointer flex-wrap items-center gap-3 px-3 py-2"
        data-testid="trades-group-header"
      >
        <div className="min-w-0 flex-1 truncate text-sm font-semibold" title={label}>
          {label}
        </div>
        <Badge variant="outline" className="shrink-0 font-mono text-[10px]">
          {legCountLabel}
        </Badge>
        {group.feesPending && (
          <Badge
            variant="secondary"
            className="shrink-0 text-[10px]"
            title="Some legs have no commission report yet"
          >
            fees pending
          </Badge>
        )}
        <div className="flex shrink-0 gap-4 font-mono text-xs tabular-nums">
          <span className="text-muted-foreground">
            gross{" "}
            <span className={pnlClass(group.grossRealized)} data-testid="trades-group-gross">
              {fmtUsd(group.grossRealized)}
            </span>
          </span>
          <span className="text-muted-foreground">
            fees{" "}
            <span className="text-foreground" data-testid="trades-group-fees">
              {fmtUsd(group.fees)}
            </span>
          </span>
          <span className="text-muted-foreground">
            net{" "}
            <span className={pnlClass(group.netPnL)} data-testid="trades-group-net">
              {fmtUsd(group.netPnL)}
            </span>
          </span>
        </div>
      </summary>
      <div className="overflow-x-auto px-3 pb-2">
        <table className="w-full text-left">
          <thead className="text-muted-foreground text-[10px] uppercase">
            <tr>
              <th className="px-3 py-1 font-medium">Time (ET)</th>
              <th className="px-3 py-1 font-medium">Side</th>
              <th className="px-3 py-1 text-right font-medium">Qty</th>
              <th className="px-3 py-1 text-right font-medium">Avg Px</th>
              <th className="px-3 py-1 text-right font-medium">Comm</th>
              <th className="px-3 py-1 text-right font-medium">Realized</th>
              <th className="px-3 py-1 text-right font-medium">Ccy</th>
            </tr>
          </thead>
          <tbody>
            {group.legs.map((leg) => (
              <TradesLeg key={leg.exec_id} leg={leg} />
            ))}
          </tbody>
        </table>
      </div>
    </details>
  )
}
