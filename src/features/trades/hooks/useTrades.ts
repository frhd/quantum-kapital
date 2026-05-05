/**
 * Phase 3 — data hook for the Today's Trades panel.
 *
 * The repo doesn't install `@tanstack/react-query` (see
 * `loop/plan/QUESTIONS.md`); this hook follows the existing
 * `useState` + `useCallback` pattern from `useCandidates` /
 * `useAccountData` and adds a manual `visibilitychange`/`focus`
 * listener so the documented "refetch when the window comes back into
 * focus" behaviour still applies.
 */

import { useCallback, useEffect, useState } from "react"
import { ibkrApi } from "../../../shared/api/ibkr"
import type { ExecutionRow } from "../types"

export interface UseTradesResult {
  rows: ExecutionRow[]
  loading: boolean
  refreshing: boolean
  error: string | null
  refresh: () => Promise<void>
}

export function useTrades(date: string, account?: string | null): UseTradesResult {
  const [rows, setRows] = useState<ExecutionRow[]>([])
  const [loading, setLoading] = useState(true)
  const [refreshing, setRefreshing] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const refresh = useCallback(async () => {
    setRefreshing(true)
    setError(null)
    try {
      const fetched = await ibkrApi.trades.listForDate(date, account ?? null)
      setRows(fetched)
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

  // Manual stand-in for React Query's `refetchOnWindowFocus`: refetch
  // whenever the window regains focus / becomes visible. The `focus`
  // event fires for traditional focus changes; `visibilitychange`
  // covers the Tauri tray-collapse case.
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

  return { rows, loading, refreshing, error, refresh }
}
