/**
 * Phase 4 — candidate-universe data hook.
 *
 * Loads the inbox of staged candidates (and optionally promoted ones
 * for audit), exposes a `refresh` that re-queries the backing list,
 * and a `triggerRefresh` that asks the backend to run sentiment-surge
 * + decay out of band before re-querying.
 */

import { useCallback, useEffect, useState } from "react"
import { ibkrApi } from "../../../shared/api/ibkr"
import type { Candidate, CandidatesQuery } from "../types"

interface UseCandidatesResult {
  candidates: Candidate[]
  loading: boolean
  refreshing: boolean
  error: string | null
  refresh: () => Promise<void>
  triggerRefresh: () => Promise<void>
}

export function useCandidates(query: CandidatesQuery): UseCandidatesResult {
  const [candidates, setCandidates] = useState<Candidate[]>([])
  const [loading, setLoading] = useState(true)
  const [refreshing, setRefreshing] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const queryKey = JSON.stringify(query)

  const refresh = useCallback(async () => {
    try {
      setError(null)
      const rows = await ibkrApi.candidates.list(query)
      setCandidates(rows)
    } catch (e) {
      setError(typeof e === "string" ? e : (e as Error).message)
    } finally {
      setLoading(false)
    }
    // We intentionally key on the stringified query so a stable identity
    // doesn't refetch when the caller re-renders.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [queryKey])

  const triggerRefresh = useCallback(async () => {
    setRefreshing(true)
    try {
      await ibkrApi.candidates.refreshNow()
    } catch (e) {
      setError(typeof e === "string" ? e : (e as Error).message)
    } finally {
      setRefreshing(false)
    }
    await refresh()
  }, [refresh])

  useEffect(() => {
    setLoading(true)
    void refresh()
  }, [refresh])

  return { candidates, loading, refreshing, error, refresh, triggerRefresh }
}
