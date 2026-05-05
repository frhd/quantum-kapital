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
    const onActive = () => {
      void refresh()
    }
    window.addEventListener("focus", onActive)
    document.addEventListener("visibilitychange", () => {
      if (document.visibilityState === "visible") onActive()
    })
    return () => {
      window.removeEventListener("focus", onActive)
      document.removeEventListener("visibilitychange", onActive)
    }
  }, [refresh])

  return { playbook, loading, refreshing, error, refresh }
}
