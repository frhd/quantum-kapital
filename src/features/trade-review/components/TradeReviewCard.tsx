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

import { useState } from "react"

import { assessmentsApi } from "../../../shared/api/assessments"
import { Button } from "../../../shared/components/ui/button"
import { Card, CardContent, CardHeader, CardTitle } from "../../../shared/components/ui/card"
import { DatePicker } from "../../../shared/components/DatePicker"
import { Skeleton } from "../../../shared/components/ui/skeleton"

import type { TradeReview } from "../types"
import { useTradeReview } from "../hooks/useTradeReview"
import { EmptyTradeReview } from "./EmptyTradeReview"
import { EquityCurve } from "./EquityCurve"
import { GradeBadge } from "./GradeBadge"
import { PopulatedReview } from "./PopulatedReview"
import { RiskMetricsPanel } from "./RiskMetricsPanel"

/** v1 rows surface the legacy GradeBadge with a `v1` chip; v2 rows
 *  surface `score_v2` and `discipline_v2` as separate numbers (master
 *  commitment: never sum them for ranking). */
function ScoreSurface({ review }: { review: TradeReview | null | undefined }) {
  if (!review) return null
  if (review.formula_version === "v1" && review.grade) {
    return (
      <span className="flex items-center gap-1">
        <GradeBadge grade={review.grade} score={review.grade_score ?? 0} />
        <span className="text-muted-foreground border-border rounded border px-1.5 py-0.5 text-[10px] uppercase">
          v1
        </span>
      </span>
    )
  }
  return (
    <span className="flex items-center gap-3 text-xs">
      <span className="border-border bg-secondary/40 rounded border px-2 py-0.5">
        <span className="text-muted-foreground mr-1 uppercase">R-edge</span>
        <span className="font-mono">
          {review.score_v2 != null ? review.score_v2.toFixed(2) : "—"}
        </span>
      </span>
      <span className="border-border bg-secondary/40 rounded border px-2 py-0.5">
        <span className="text-muted-foreground mr-1 uppercase">Discipline</span>
        <span
          className={`font-mono ${
            (review.discipline_v2 ?? 0) >= 0 ? "text-green-500" : "text-red-500"
          }`}
        >
          {review.discipline_v2 != null
            ? (review.discipline_v2 >= 0 ? "+" : "") + review.discipline_v2.toFixed(0)
            : "—"}
        </span>
      </span>
    </span>
  )
}

const ET_DATE_FMT = new Intl.DateTimeFormat("en-CA", { timeZone: "America/New_York" })

function todayEt(): string {
  return ET_DATE_FMT.format(new Date())
}

export interface TradeReviewCardProps {
  date?: string
  account?: string | null
}

export function TradeReviewCard({ date: dateProp, account }: TradeReviewCardProps = {}) {
  const [date, setDate] = useState(dateProp ?? todayEt())
  const { review, loading, refreshing, error, refresh } = useTradeReview(date, account ?? null)
  const [regenerating, setRegenerating] = useState(false)
  const [regenerateError, setRegenerateError] = useState<string | null>(null)

  const handleRegenerate = async () => {
    const ok = window.confirm(
      `Re-run the trade review for ${date}? This burns an LLM call and overwrites the existing review.`,
    )
    if (!ok) return
    setRegenerating(true)
    setRegenerateError(null)
    try {
      await assessmentsApi.generateTradeReview(date, { account: account ?? null })
      await refresh()
    } catch (e) {
      setRegenerateError(typeof e === "string" ? e : (e as Error).message)
    } finally {
      setRegenerating(false)
    }
  }

  return (
    <Card className="border-border bg-card/50">
      <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
        <div>
          <CardTitle className="flex items-center gap-3 text-base font-semibold">
            <span>Trade Review</span>
            <ScoreSurface review={review} />
          </CardTitle>
          <p className="text-muted-foreground mt-1 text-xs">
            Deterministic grade + LLM narrative for {date} (ET).
          </p>
        </div>
        <div className="flex items-center gap-2">
          <DatePicker value={date} onChange={setDate} ariaLabel="Trading day" />
          {review && (
            <Button
              size="sm"
              variant="outline"
              onClick={() => void handleRegenerate()}
              disabled={regenerating || refreshing}
              className="h-8 px-3 text-xs"
            >
              {regenerating ? "Regenerating…" : "Regenerate"}
            </Button>
          )}
          <Button
            size="sm"
            variant="ghost"
            onClick={() => void refresh()}
            disabled={refreshing || regenerating}
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
          <div className="space-y-3">
            {regenerateError && (
              <p className="text-destructive text-sm" role="alert">
                Failed to regenerate: {regenerateError}
              </p>
            )}
            {review.formula_version === "v2" && (
              <>
                <RiskMetricsPanel metrics={review.risk_metrics ?? null} />
                {review.equity_curve && review.equity_curve.length > 0 && (
                  <EquityCurve points={review.equity_curve} caption="Daily equity (trade flow)" />
                )}
              </>
            )}
            <PopulatedReview review={review} />
          </div>
        ) : (
          <EmptyTradeReview
            date={date}
            onGenerate={async () => {
              const generated = await assessmentsApi.generateTradeReview(date, {
                account: account ?? null,
              })
              if (generated === null) {
                throw new Error(`No fills found for ${date} on this account — nothing to review.`)
              }
              await refresh()
            }}
          />
        )}
      </CardContent>
    </Card>
  )
}
