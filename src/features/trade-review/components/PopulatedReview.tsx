import { useMemo } from "react"

import { MarkdownBody } from "../../research/components/MarkdownBody"
import type { TradeReview } from "../types"
import { LegObservationsList } from "./LegObservationsList"
import { TradeReviewShareCard } from "./TradeReviewShareCard"

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

export function PopulatedReview({ review }: { review: TradeReview }) {
  const bySymbol = useMemo(() => {
    return Object.entries(review.summary.by_symbol).sort(([, a], [, b]) => b - a)
  }, [review.summary.by_symbol])

  return (
    <div className="space-y-4">
      <TradeReviewShareCard review={review} date={review.date} />

      {bySymbol.length > 0 && (
        <div className="border-border bg-background/40 rounded-md border p-3">
          <h3 className="text-muted-foreground mb-2 text-[10px] font-semibold tracking-wider uppercase">
            P&L by symbol
          </h3>
          <ul className="grid grid-cols-2 gap-x-4 gap-y-1 text-xs sm:grid-cols-3">
            {bySymbol.map(([sym, pnl]) => (
              <li key={sym} className="flex items-center justify-between">
                <span className="font-semibold">{sym}</span>
                <span className={`font-mono tabular-nums ${pnlClass(pnl)}`}>{fmtUsd(pnl)}</span>
              </li>
            ))}
          </ul>
        </div>
      )}

      {review.narrative_md.trim().length > 0 && <MarkdownBody markdown={review.narrative_md} />}

      <LegObservationsList items={review.leg_observations} />
    </div>
  )
}
