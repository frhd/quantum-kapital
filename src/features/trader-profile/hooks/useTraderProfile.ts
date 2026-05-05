/**
 * Phase 7 — data hook for the Trader Profile dashboard.
 *
 * Pure SQL aggregator over `day_reviews`; cheap to refresh, so this
 * follows the same focus/visibility refetch as the other assessment
 * hooks. The default window is 30 days (matches the MCP tool default).
 */

import { useCallback, useEffect, useState } from "react"

import { assessmentsApi } from "../../../shared/api/assessments"
import type { TraderProfile } from "../types"

export interface UseTraderProfileResult {
  profile: TraderProfile | null
  loading: boolean
  refreshing: boolean
  error: string | null
  refresh: () => Promise<void>
}

export function useTraderProfile(
  windowDays: number = 30,
  account?: string | null,
): UseTraderProfileResult {
  const [profile, setProfile] = useState<TraderProfile | null>(null)
  const [loading, setLoading] = useState(true)
  const [refreshing, setRefreshing] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const refresh = useCallback(async () => {
    setRefreshing(true)
    setError(null)
    try {
      const fetched = await assessmentsApi.getTraderProfile({
        windowDays,
        account: account ?? null,
      })
      setProfile(fetched)
    } catch (e) {
      setError(typeof e === "string" ? e : (e as Error).message)
    } finally {
      setLoading(false)
      setRefreshing(false)
    }
  }, [windowDays, account])

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

  return { profile, loading, refreshing, error, refresh }
}
