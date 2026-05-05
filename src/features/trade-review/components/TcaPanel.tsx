import { useEffect, useMemo, useState } from "react"

import {
  type AttributionRow,
  type SlippageDistributionRow,
  formatBucketRange,
  formatPnl,
  formatSlippageBps,
  formatStrategy,
  tcaGetAttribution,
  tcaGetSlippageDistribution,
} from "@/shared/api/tca"

interface Props {
  /** Inclusive ET trading-day range, ISO `YYYY-MM-DD`. */
  dateFrom: string
  dateTo: string
  account?: string
}

interface PanelState {
  status: "idle" | "loading" | "ready" | "error"
  attribution: AttributionRow[]
  distribution: SlippageDistributionRow[]
  errorMsg?: string
}

const INITIAL: PanelState = {
  status: "idle",
  attribution: [],
  distribution: [],
}

export function TcaPanel({ dateFrom, dateTo, account }: Props) {
  const [state, setState] = useState<PanelState>(INITIAL)

  useEffect(() => {
    let cancelled = false
    setState({ ...INITIAL, status: "loading" })
    Promise.all([
      tcaGetAttribution(dateFrom, dateTo, account),
      tcaGetSlippageDistribution(dateFrom, dateTo, account),
    ])
      .then(([attribution, distribution]) => {
        if (cancelled) return
        setState({ status: "ready", attribution, distribution })
      })
      .catch((e: unknown) => {
        if (cancelled) return
        setState({
          ...INITIAL,
          status: "error",
          errorMsg: e instanceof Error ? e.message : String(e),
        })
      })
    return () => {
      cancelled = true
    }
  }, [dateFrom, dateTo, account])

  if (state.status === "loading" || state.status === "idle") {
    return <div className="text-muted-foreground text-xs">Loading TCA…</div>
  }
  if (state.status === "error") {
    return <div className="text-xs text-red-400">TCA error: {state.errorMsg}</div>
  }
  const hasAttribution = state.attribution.length > 0
  const hasDistribution = state.distribution.length > 0
  if (!hasAttribution && !hasDistribution) {
    return <div className="text-muted-foreground text-xs">No fills in the selected window.</div>
  }
  return (
    <div className="space-y-4">
      {hasAttribution && <AttributionTable rows={state.attribution} />}
      {hasDistribution && <SlippageHistogram rows={state.distribution} />}
    </div>
  )
}

function AttributionTable({ rows }: { rows: AttributionRow[] }) {
  const sorted = useMemo(() => [...rows].sort((a, b) => b.n_trades - a.n_trades), [rows])
  return (
    <div className="border-border bg-background/40 rounded-md border p-3">
      <h3 className="text-muted-foreground mb-2 text-[10px] font-semibold tracking-wider uppercase">
        Attribution by strategy
      </h3>
      <table className="w-full text-xs">
        <thead>
          <tr className="text-muted-foreground border-border border-b">
            <th className="py-1 text-left">Strategy</th>
            <th className="py-1 text-right">Trades</th>
            <th className="py-1 text-right">Net P&L</th>
            <th className="py-1 text-right">Avg Slip</th>
          </tr>
        </thead>
        <tbody>
          {sorted.map((r) => (
            <tr key={r.strategy ?? "__none__"} className="border-border/50 border-b last:border-0">
              <td className="py-1 font-semibold">{formatStrategy(r.strategy)}</td>
              <td className="py-1 text-right font-mono tabular-nums">{r.n_trades}</td>
              <td className={`py-1 text-right font-mono tabular-nums ${pnlClass(r.net_pnl_cents)}`}>
                {formatPnl(r.net_pnl_cents)}
              </td>
              <td className="py-1 text-right font-mono tabular-nums">
                {r.n_with_slippage > 0 ? formatSlippageBps(r.avg_slippage_bps) : "—"}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}

function SlippageHistogram({ rows }: { rows: SlippageDistributionRow[] }) {
  return (
    <div className="border-border bg-background/40 rounded-md border p-3">
      <h3 className="text-muted-foreground mb-2 text-[10px] font-semibold tracking-wider uppercase">
        Slippage distribution
      </h3>
      <div className="space-y-3">
        {rows.map((row) => {
          const max = Math.max(...row.buckets.map((b) => b.n), 1)
          return (
            <div key={`${row.strategy ?? "__none__"}-${row.liquidity_bucket}`}>
              <div className="text-muted-foreground mb-1 text-[10px] tracking-wider uppercase">
                {formatStrategy(row.strategy)}
              </div>
              <ul className="space-y-0.5">
                {row.buckets.map((b) => (
                  <li key={`${b.lower_bps}-${b.upper_bps}`} className="flex items-center gap-2">
                    <span className="text-muted-foreground w-14 text-right font-mono text-[10px] tabular-nums">
                      {formatBucketRange(b)}
                    </span>
                    <div className="bg-secondary/40 relative h-3 flex-1 overflow-hidden rounded">
                      <div
                        className="bg-primary/70 h-full"
                        style={{ width: `${(b.n / max) * 100}%` }}
                      />
                    </div>
                    <span className="w-8 text-right font-mono text-[10px] tabular-nums">{b.n}</span>
                  </li>
                ))}
              </ul>
            </div>
          )
        })}
      </div>
    </div>
  )
}

function pnlClass(cents: number): string {
  if (cents > 0) return "text-green-400"
  if (cents < 0) return "text-red-400"
  return "text-muted-foreground"
}
