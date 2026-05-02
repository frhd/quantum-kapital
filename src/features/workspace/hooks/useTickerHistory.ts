import { useCallback, useEffect, useMemo, useState } from "react"

import { ibkrApi } from "../../../shared/api/ibkr"
import type { PredictionWithOutcome } from "../../eval/types"

export interface TickerHistorySummary {
  total: number
  /** Predictions whose evaluation window has not closed yet (no
   *  outcome row at all). The accuracy headline must distinguish
   *  these from resolved misses. */
  pending: number
  /** Outcomes the eval harness recorded as `skipped` — out of scope
   *  for accuracy math but still worth surfacing for completeness. */
  skipped: number
  /** Outcomes the eval harness could not classify
   *  (`OutcomeClass::Unparseable`). Excluded from the accuracy
   *  denominator. */
  unparseable: number
  /** Resolved + scoreable predictions: total − pending − skipped −
   *  unparseable. */
  scoreable: number
  /** `hit_target + hit_entry`. */
  hits: number
  /** `hits / scoreable`, or null when `scoreable === 0`. */
  hitRate: number | null
}

interface UseTickerHistoryResult {
  rows: PredictionWithOutcome[]
  summary: TickerHistorySummary
  loading: boolean
  error: string | null
  windowDays: number
  setWindowDays: (n: number) => void
  refresh: () => Promise<void>
}

const DEFAULT_WINDOW_DAYS = 90

/**
 * Workspace Phase 3 — composes `eval.predictionHistory(symbol)` and
 * derives a small accuracy summary for the History panel headline.
 *
 * Sticking with a per-symbol sibling hook (rather than extending
 * `useEvalDashboard` with a `symbol` arg) keeps the global eval tab's
 * blast radius unchanged. Only the underlying API call is shared.
 */
export function useTickerHistory(
  symbol: string | null,
  initialWindowDays: number = DEFAULT_WINDOW_DAYS,
): UseTickerHistoryResult {
  const [rows, setRows] = useState<PredictionWithOutcome[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [windowDays, setWindowDays] = useState<number>(initialWindowDays)

  const refresh = useCallback(async () => {
    if (!symbol) {
      setRows([])
      setError(null)
      return
    }
    setLoading(true)
    setError(null)
    try {
      const out = await ibkrApi.eval.predictionHistory(symbol, windowDays)
      setRows(out)
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
      setRows([])
    } finally {
      setLoading(false)
    }
  }, [symbol, windowDays])

  useEffect(() => {
    void refresh()
  }, [refresh])

  const summary = useMemo<TickerHistorySummary>(() => deriveSummary(rows), [rows])

  return { rows, summary, loading, error, windowDays, setWindowDays, refresh }
}

function deriveSummary(rows: PredictionWithOutcome[]): TickerHistorySummary {
  let pending = 0
  let skipped = 0
  let unparseable = 0
  let hits = 0
  let scoreable = 0
  for (const row of rows) {
    if (!row.outcome) {
      pending += 1
      continue
    }
    switch (row.outcome.outcome_class) {
      case "skipped":
        skipped += 1
        break
      case "unparseable":
        unparseable += 1
        break
      case "hit_target":
      case "hit_entry":
        hits += 1
        scoreable += 1
        break
      default:
        scoreable += 1
        break
    }
  }
  const hitRate = scoreable === 0 ? null : hits / scoreable
  return {
    total: rows.length,
    pending,
    skipped,
    unparseable,
    scoreable,
    hits,
    hitRate,
  }
}
