/**
 * Phase 3 — fetch latest-per-source social sentiment for a symbol.
 *
 * Polls every 5 minutes. The widget on the analysis view consumes the
 * returned rows + loading/error state to render a one-row, three-source
 * snapshot. `refresh` triggers an out-of-band `social_refresh_now` so a
 * manual click bypasses the scheduler cooldown.
 */

import { useCallback, useEffect, useState } from "react"
import { ibkrApi } from "../../../shared/api/ibkr"
import type { SocialSentimentRow } from "../types"

const POLL_INTERVAL_MS = 5 * 60 * 1000

interface UseSocialSentimentResult {
  rows: SocialSentimentRow[]
  loading: boolean
  error: string | null
  refresh: () => Promise<void>
  refreshing: boolean
}

export function useSocialSentiment(symbol: string | null): UseSocialSentimentResult {
  const [rows, setRows] = useState<SocialSentimentRow[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [refreshing, setRefreshing] = useState(false)

  const fetchOnce = useCallback(async (sym: string, opts?: { silent?: boolean }) => {
    if (!opts?.silent) setLoading(true)
    try {
      const result = await ibkrApi.socialSentiment.getLatest(sym)
      setRows(result)
      setError(null)
    } catch (err) {
      setError(typeof err === "string" ? err : ((err as Error)?.message ?? "fetch failed"))
    } finally {
      if (!opts?.silent) setLoading(false)
    }
  }, [])

  useEffect(() => {
    if (!symbol) {
      setRows([])
      setError(null)
      return
    }
    let cancelled = false
    setRows([])
    setError(null)

    const tick = () => {
      if (cancelled) return
      fetchOnce(symbol).catch(() => {
        /* error already captured */
      })
    }

    tick()
    const id = window.setInterval(tick, POLL_INTERVAL_MS)
    return () => {
      cancelled = true
      window.clearInterval(id)
    }
  }, [symbol, fetchOnce])

  const refresh = useCallback(async () => {
    if (!symbol) return
    setRefreshing(true)
    try {
      // Trigger an out-of-band scheduler tick for the current symbol +
      // anything else on the watchlist; then re-read latest-per-source.
      await ibkrApi.socialSentiment.refreshNow([symbol])
      await fetchOnce(symbol, { silent: true })
    } catch (err) {
      setError(typeof err === "string" ? err : ((err as Error)?.message ?? "refresh failed"))
    } finally {
      setRefreshing(false)
    }
  }, [symbol, fetchOnce])

  return { rows, loading, error, refresh, refreshing }
}
