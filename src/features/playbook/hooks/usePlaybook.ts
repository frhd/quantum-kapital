/**
 * Phase 7 — data hook for the Today's Playbook panel.
 *
 * Mirrors `useTradeReview` and `useTrades`. The `morning_sweep`
 * cron writes the row at 07:00 ET; manual focus/visibility refetch
 * lets the user pick up the row without a hard reload.
 */

import { useCallback, useEffect, useState } from "react"

import { assessmentsApi } from "../../../shared/api/assessments"
import type { Playbook } from "../types"

export interface UsePlaybookResult {
  playbook: Playbook | null
  loading: boolean
  refreshing: boolean
  error: string | null
  refresh: () => Promise<void>
}

export function usePlaybook(date: string, account?: string | null): UsePlaybookResult {
  const [playbook, setPlaybook] = useState<Playbook | null>(null)
  const [loading, setLoading] = useState(true)
  const [refreshing, setRefreshing] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const refresh = useCallback(async () => {
    setRefreshing(true)
    setError(null)
    try {
      const fetched = await assessmentsApi.getPlaybook(date, { account: account ?? null })
      setPlaybook(fetched)
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

  return { playbook, loading, refreshing, error, refresh }
}
