/**
 * Phase 3 — Today's Trades top-level page.
 *
 * Date picker (defaults to today, ET) → summary banner →
 * symbol/option-key grouped list of fills. The page targets the
 * end-of-day "how did I do?" review pattern: open it after the close,
 * scan the symbol headers for net P&L, expand a group to read the leg
 * sequence in execution order.
 */

import { useMemo, useState } from "react"
import { Card, CardContent, CardHeader, CardTitle } from "../../../shared/components/ui/card"
import { Button } from "../../../shared/components/ui/button"
import { Skeleton } from "../../../shared/components/ui/skeleton"
import { Badge } from "../../../shared/components/ui/badge"
import { DatePicker } from "../../../shared/components/DatePicker"
import { useTrades } from "../hooks/useTrades"
import { groupExecutions, summariseGroups } from "../groupExecutions"
import { TradesGroup } from "./TradesGroup"
import type { TradesSummary } from "../types"

const ET_DATE_FMT = new Intl.DateTimeFormat("en-CA", { timeZone: "America/New_York" })

function todayEt(): string {
  // en-CA renders as YYYY-MM-DD; the formatter has timeZone applied so
  // the host TZ doesn't shift the date. Matches the backend's ET
  // trading-day convention.
  return ET_DATE_FMT.format(new Date())
}

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

function SummaryBanner({ summary, date }: { summary: TradesSummary; date: string }) {
  return (
    <div
      className="border-border bg-background/40 grid grid-cols-2 gap-3 rounded-md border p-3 sm:grid-cols-4"
      data-testid="trades-summary"
    >
      <Cell label="Fills" value={String(summary.fills)} />
      <Cell
        label="Gross"
        value={fmtUsd(summary.grossRealized)}
        valueClass={pnlClass(summary.grossRealized)}
      />
      <Cell label="Fees" value={fmtUsd(summary.fees)} pendingBadge={summary.feesPending} />
      <Cell label="Net" value={fmtUsd(summary.netPnL)} valueClass={pnlClass(summary.netPnL)} />
      <p className="text-muted-foreground col-span-2 text-[10px] sm:col-span-4">
        Trading day {date} (ET) · realized P&L only · USD
      </p>
    </div>
  )
}

function Cell({
  label,
  value,
  valueClass,
  pendingBadge,
}: {
  label: string
  value: string
  valueClass?: string
  pendingBadge?: boolean
}) {
  return (
    <div>
      <div className="text-muted-foreground text-[10px] tracking-wider uppercase">{label}</div>
      <div className="flex items-baseline gap-2">
        <div className={`font-mono text-base font-semibold tabular-nums ${valueClass ?? ""}`}>
          {value}
        </div>
        {pendingBadge && (
          <Badge variant="secondary" className="text-[10px]">
            pending
          </Badge>
        )}
      </div>
    </div>
  )
}

export interface TradesPageProps {
  /** Optional callback to navigate to the Trade Review page for the
   *  selected date. The Trades panel surfaces a "Review →" link in
   *  its header that calls this — wired by the app shell so the
   *  feature folder doesn't need to know about the page router. */
  onOpenReview?: (date: string) => void
}

export function TradesPage({ onOpenReview }: TradesPageProps = {}) {
  const [date, setDate] = useState(todayEt())
  const { rows, loading, refreshing, error, refresh } = useTrades(date)
  const groups = useMemo(() => groupExecutions(rows), [rows])
  const summary = useMemo(() => summariseGroups(groups), [groups])
  const isToday = date === todayEt()

  return (
    <Card className="border-border bg-card/50">
      <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
        <div>
          <CardTitle className="text-base font-semibold">Today's Trades</CardTitle>
          <p className="text-muted-foreground mt-1 text-xs">
            Per-leg IBKR fills with realized P&L and commissions, grouped by symbol.
          </p>
        </div>
        <div className="flex items-center gap-2">
          {onOpenReview && (
            <Button
              size="sm"
              variant="ghost"
              onClick={() => onOpenReview(date)}
              className="h-8 px-3 text-xs"
            >
              Review →
            </Button>
          )}
          <DatePicker value={date} onChange={setDate} ariaLabel="Trading day" />
          <Button
            size="sm"
            variant="ghost"
            onClick={() => void refresh()}
            disabled={refreshing}
            className="h-8 px-3 text-xs"
          >
            {refreshing ? "Refreshing…" : "Refresh"}
          </Button>
        </div>
      </CardHeader>
      <CardContent className="space-y-4">
        <SummaryBanner summary={summary} date={date} />

        {error ? (
          <p className="text-destructive text-sm">Failed to load fills: {error}</p>
        ) : loading && groups.length === 0 ? (
          <div className="space-y-2">
            {[0, 1, 2].map((i) => (
              <Skeleton key={i} className="bg-secondary h-12" />
            ))}
          </div>
        ) : groups.length === 0 ? (
          <div className="text-muted-foreground space-y-1 py-6 text-center text-sm">
            <p>No fills for {date}.</p>
            {!isToday && (
              <p className="text-xs">
                Fills are captured opportunistically while the app is running. IBKR doesn't return
                prior trading days, so any day the app wasn't open during market hours stays empty.
              </p>
            )}
          </div>
        ) : (
          <div className="space-y-2">
            {groups.map((g) => (
              <TradesGroup
                key={`${g.symbol}-${g.optionKey ? `${g.optionKey.expiry}-${g.optionKey.strike}-${g.optionKey.right}` : "stk"}`}
                group={g}
              />
            ))}
          </div>
        )}
      </CardContent>
    </Card>
  )
}
