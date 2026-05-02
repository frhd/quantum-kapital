import { useCallback, useEffect, useState } from "react"

import { ibkrApi } from "../../../shared/api/ibkr"
import type { CachedTickerNews, NewsVerdict } from "../types"

interface UseTickerNewsResult {
  data: CachedTickerNews | null
  loading: boolean
  error: string | null
  /** Best-effort decode of `verdict_json`. `null` when the column was
   *  null OR the JSON is not a recognizable verdict object. */
  verdict: NewsVerdict | null
  refresh: () => Promise<void>
}

/**
 * Workspace Phase 3 — pulls the cached news payload for `symbol` from
 * the `news_cache` table. Cache-only by design: the producer
 * (`IbkrNewsProvider` and friends) writes new rows on its own
 * schedule. Surfacing `fetched_at_unix` lets the panel render a
 * staleness indicator so the user can judge how recent the rows are.
 *
 * Returns `data === null` while the first fetch is in flight so the
 * panel can render a loading state without flashing an empty-state
 * card.
 */
export function useTickerNews(symbol: string | null): UseTickerNewsResult {
  const [data, setData] = useState<CachedTickerNews | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const refresh = useCallback(async () => {
    if (!symbol) {
      setData(null)
      setError(null)
      return
    }
    setLoading(true)
    setError(null)
    try {
      const result = await ibkrApi.tracker.getCachedNews(symbol)
      setData(result)
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
      setData(null)
    } finally {
      setLoading(false)
    }
  }, [symbol])

  useEffect(() => {
    void refresh()
  }, [refresh])

  return { data, loading, error, verdict: decodeVerdict(data?.verdict_json ?? null), refresh }
}

function decodeVerdict(raw: string | null): NewsVerdict | null {
  if (!raw) return null
  try {
    const parsed = JSON.parse(raw) as unknown
    if (parsed && typeof parsed === "object") {
      return parsed as NewsVerdict
    }
    return null
  } catch {
    return null
  }
}
