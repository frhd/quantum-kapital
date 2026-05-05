/**
 * Phase 7 — Trade Review card.
 *
 * Renders the persisted `day_reviews` row for a given trading day:
 * deterministic A/B/C/D/F grade, leg-summary numbers, behavioral tag
 * chips, the LLM-authored markdown narrative, and the optional leg
 * observations list. Distinguishes loading / error / empty / populated
 * states. Reviews are written by `agent/eod_review.py` at 17:00 ET via
 * the `write_trade_review` MCP write rail.
 */

import { useMemo, useState } from "react"

import { Button } from "../../../shared/components/ui/button"
import { Card, CardContent, CardHeader, CardTitle } from "../../../shared/components/ui/card"
import { DatePicker } from "../../../shared/components/DatePicker"
import { Skeleton } from "../../../shared/components/ui/skeleton"

import { MarkdownBody } from "../../research/components/MarkdownBody"
import { useTradeReview } from "../hooks/useTradeReview"
import type { LegSummary, TradeReview } from "../types"
import { BehavioralTagChip } from "./BehavioralTagChip"
import { EmptyTradeReview } from "./EmptyTradeReview"
import { GradeBadge } from "./GradeBadge"
import { LegObservationsList } from "./LegObservationsList"

const ET_DATE_FMT = new Intl.DateTimeFormat("en-CA", { timeZone: "America/New_York" })

function todayEt(): string {
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

function fmtPct(v: number | null | undefined): string {
  if (v === null || v === undefined) return "—"
  return `${(v * 100).toFixed(0)}%`
}

function SummaryRow({ summary }: { summary: LegSummary }) {
  return (
    <div
      className="border-border bg-background/40 grid grid-cols-2 gap-3 rounded-md border p-3 sm:grid-cols-4"
      data-testid="review-summary"
    >
      <Cell
        label="Net P&L"
        value={fmtUsd(summary.net_pnl)}
        valueClass={pnlClass(summary.net_pnl)}
      />
      <Cell
        label="Gross"
        value={fmtUsd(summary.gross_pnl)}
        valueClass={pnlClass(summary.gross_pnl)}
      />
      <Cell label="Commissions" value={fmtUsd(summary.commissions_total)} />
      <Cell label="Win rate" value={fmtPct(summary.win_rate ?? null)} />
      <Cell label="Round trips" value={String(summary.n_round_trips)} />
      <Cell label="Carryover" value={String(summary.n_carryover)} />
    </div>
  )
}

function Cell({ label, value, valueClass }: { label: string; value: string; valueClass?: string }) {
  return (
    <div>
      <div className="text-muted-foreground text-[10px] tracking-wider uppercase">{label}</div>
      <div className={`font-mono text-base font-semibold tabular-nums ${valueClass ?? ""}`}>
        {value}
      </div>
    </div>
  )
}

function PopulatedReview({ review }: { review: TradeReview }) {
  const bySymbol = useMemo(() => {
    return Object.entries(review.summary.by_symbol).sort(([, a], [, b]) => b - a)
  }, [review.summary.by_symbol])

  return (
    <div className="space-y-4">
      <SummaryRow summary={review.summary} />

      {review.behavioral_tags.length > 0 && (
        <div data-testid="review-tags" className="flex flex-wrap gap-1.5">
          {review.behavioral_tags.map((tag) => (
            <BehavioralTagChip key={tag} tag={tag} />
          ))}
        </div>
      )}

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

export interface TradeReviewCardProps {
  date?: string
  account?: string | null
}

export function TradeReviewCard({ date: dateProp, account }: TradeReviewCardProps = {}) {
  const [date, setDate] = useState(dateProp ?? todayEt())
  const { review, loading, refreshing, error, refresh } = useTradeReview(date, account ?? null)

  return (
    <Card className="border-border bg-card/50">
      <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
        <div>
          <CardTitle className="flex items-center gap-3 text-base font-semibold">
            <span>Trade Review</span>
            {review && <GradeBadge grade={review.grade} score={review.grade_score} />}
          </CardTitle>
          <p className="text-muted-foreground mt-1 text-xs">
            Deterministic grade + LLM narrative for {date} (ET).
          </p>
        </div>
        <div className="flex items-center gap-2">
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
      <CardContent>
        {error ? (
          <p className="text-destructive text-sm">Failed to load review: {error}</p>
        ) : loading ? (
          <div className="space-y-2">
            <Skeleton className="bg-secondary h-16" />
            <Skeleton className="bg-secondary h-24" />
          </div>
        ) : review ? (
          <PopulatedReview review={review} />
        ) : (
          <EmptyTradeReview date={date} />
        )}
      </CardContent>
    </Card>
  )
}
