/**
 * Phase 7 — data hook for the Trade Review card.
 *
 * Mirrors `useTrades` in pattern: `useState` + `useEffect` + a manual
 * focus/visibility refetch (the assessment cron writes the row at
 * 17:00 ET, so the user might switch back to the app a few minutes
 * later expecting fresh data).
 */

import { useCallback, useEffect, useState } from "react"

import { assessmentsApi } from "../../../shared/api/assessments"
import type { TradeReview } from "../types"

export interface UseTradeReviewResult {
  review: TradeReview | null
  loading: boolean
  refreshing: boolean
  error: string | null
  refresh: () => Promise<void>
}

export function useTradeReview(date: string, account?: string | null): UseTradeReviewResult {
  const [review, setReview] = useState<TradeReview | null>(null)
  const [loading, setLoading] = useState(true)
  const [refreshing, setRefreshing] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const refresh = useCallback(async () => {
    setRefreshing(true)
    setError(null)
    try {
      const fetched = await assessmentsApi.getTradeReview(date, { account: account ?? null })
      setReview(fetched)
    } catch (e) {
      setError(typeof e === "string" ? e : (e as Error).message)
    } finally {
      setLoading(false)
      setRefreshing(false)
    }
  }, [date, account])

  useEffect(() => {
    setLoading(true)
    void refresh()
  }, [refresh])

  useEffect(() => {
    const onFocus = () => {
      void refresh()
    }
    const onVisibility = () => {
      if (document.visibilityState === "visible") void refresh()
    }
    window.addEventListener("focus", onFocus)
    document.addEventListener("visibilitychange", onVisibility)
    return () => {
      window.removeEventListener("focus", onFocus)
      document.removeEventListener("visibilitychange", onVisibility)
    }
  }, [refresh])

  return { review, loading, refreshing, error, refresh }
}
