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

  return { profile, loading, refreshing, error, refresh }
}
