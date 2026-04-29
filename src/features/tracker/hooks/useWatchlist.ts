import { useCallback, useEffect, useRef, useState } from "react"
import { ibkrApi } from "../../../shared/api/ibkr"
import type { StrategyTag, TrackedTicker, TrackerSource, TrackerStatus } from "../types"

interface AddArgs {
  symbol: string
  source: TrackerSource
  sourceMeta?: Record<string, unknown> | null
  tags: StrategyTag[]
  notes?: string | null
}

export function useWatchlist(refreshKey: number = 0) {
  const [tickers, setTickers] = useState<TrackedTicker[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const seqRef = useRef(0)

  const refresh = useCallback(async () => {
    const seq = ++seqRef.current
    setLoading(true)
    setError(null)
    try {
      const list = await ibkrApi.tracker.list()
      if (seq === seqRef.current) {
        setTickers(list)
      }
    } catch (err) {
      if (seq === seqRef.current) {
        setError(err instanceof Error ? err.message : String(err))
      }
    } finally {
      if (seq === seqRef.current) {
        setLoading(false)
      }
    }
  }, [])

  useEffect(() => {
    void refresh()
  }, [refresh, refreshKey])

  const add = useCallback(
    async (args: AddArgs) => {
      const ticker = await ibkrApi.tracker.add(args)
      await refresh()
      return ticker
    },
    [refresh],
  )

  const remove = useCallback(
    async (symbol: string) => {
      await ibkrApi.tracker.remove(symbol)
      await refresh()
    },
    [refresh],
  )

  const setTags = useCallback(
    async (symbol: string, tags: StrategyTag[]) => {
      const ticker = await ibkrApi.tracker.setTags(symbol, tags)
      await refresh()
      return ticker
    },
    [refresh],
  )

  const setStatus = useCallback(
    async (symbol: string, status: TrackerStatus, inPlayUntil?: string | null) => {
      const ticker = await ibkrApi.tracker.setStatus(symbol, status, inPlayUntil)
      await refresh()
      return ticker
    },
    [refresh],
  )

  return { tickers, loading, error, refresh, add, remove, setTags, setStatus }
}
