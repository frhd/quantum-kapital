import { useCallback, useEffect, useMemo, useRef, useState } from "react"
import { ibkrApi } from "../../../shared/api/ibkr"
import type { Alert, AlertKind } from "../types"
import type {
  SetupDetectedPayload,
  SetupInvalidatedPayload,
  TickerStatusChangedPayload,
} from "../types"

const PAGE_SIZE = 50
const MAX_ALERTS = 500

interface UseAlertsArgs {
  /** Latest SetupDetected event from useTrackerEvents — triggers a refresh. */
  lastSetupDetected: SetupDetectedPayload | null
  /** Latest SetupInvalidated event — triggers a refresh. */
  lastInvalidated: SetupInvalidatedPayload | null
  /** Status change events that may correspond to a target_hit alert. */
  lastStatusChanged: TickerStatusChangedPayload | null
  /** Active filter — `null` means show all kinds. */
  filterKind: AlertKind | null
  /** When true, hide alerts already marked seen. */
  onlyUnseen: boolean
}

export interface UseAlertsResult {
  alerts: Alert[]
  loading: boolean
  error: string | null
  unseenCount: number
  hasMore: boolean
  refresh: () => Promise<void>
  loadMore: () => Promise<void>
  markAllSeen: () => Promise<void>
  markOneSeen: (id: number) => Promise<void>
}

/**
 * Manages the alert feed. Hydrates from `tracker_list_alerts` on mount
 * and on each tracker event so freshly-recorded alerts always surface.
 * Mark-as-seen calls hit the backend and patch local state in place.
 */
export function useAlerts({
  lastSetupDetected,
  lastInvalidated,
  lastStatusChanged,
  filterKind,
  onlyUnseen,
}: UseAlertsArgs): UseAlertsResult {
  const [alerts, setAlerts] = useState<Alert[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [hasMore, setHasMore] = useState(false)
  const cancelledRef = useRef(false)

  const fetchPage = useCallback(
    async (offset: number, replace: boolean) => {
      setLoading(true)
      setError(null)
      try {
        const page = await ibkrApi.tracker.listAlerts({
          limit: PAGE_SIZE,
          offset,
          kind: filterKind,
          onlyUnseen,
        })
        if (cancelledRef.current) return
        setAlerts((prev) => {
          const base = replace ? [] : prev
          const seenIds = new Set(base.map((a) => a.id))
          const merged = [...base]
          for (const a of page) {
            if (!seenIds.has(a.id)) merged.push(a)
          }
          return merged.slice(0, MAX_ALERTS)
        })
        setHasMore(page.length >= PAGE_SIZE)
      } catch (e) {
        if (cancelledRef.current) return
        setError(e instanceof Error ? e.message : String(e))
      } finally {
        if (!cancelledRef.current) setLoading(false)
      }
    },
    [filterKind, onlyUnseen],
  )

  const refresh = useCallback(async () => {
    await fetchPage(0, true)
  }, [fetchPage])

  const loadMore = useCallback(async () => {
    if (loading || !hasMore) return
    await fetchPage(alerts.length, false)
  }, [alerts.length, fetchPage, hasMore, loading])

  // Initial load + refetch when filters change.
  useEffect(() => {
    cancelledRef.current = false
    void refresh()
    return () => {
      cancelledRef.current = true
    }
  }, [refresh])

  // Refetch when a fresh tracker event lands so the new alert row shows
  // up without manual reload.
  const lastSetupId = lastSetupDetected?.setup.id ?? null
  const lastInvalidatedId = lastInvalidated?.setup_id ?? null
  const lastStatusKey = lastStatusChanged
    ? `${lastStatusChanged.symbol}:${lastStatusChanged.from}:${lastStatusChanged.to}`
    : null
  useEffect(() => {
    if (lastSetupId === null && lastInvalidatedId === null && lastStatusKey === null) return
    void refresh()
  }, [lastSetupId, lastInvalidatedId, lastStatusKey, refresh])

  const unseenCount = useMemo(() => alerts.filter((a) => !a.seen).length, [alerts])

  const markAllSeen = useCallback(async () => {
    const unseen = alerts.filter((a) => !a.seen).map((a) => a.id)
    if (unseen.length === 0) return
    try {
      await ibkrApi.tracker.markAlertsSeen(unseen)
      setAlerts((prev) => prev.map((a) => (unseen.includes(a.id) ? { ...a, seen: true } : a)))
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    }
  }, [alerts])

  const markOneSeen = useCallback(async (id: number) => {
    try {
      await ibkrApi.tracker.markAlertsSeen([id])
      setAlerts((prev) => prev.map((a) => (a.id === id ? { ...a, seen: true } : a)))
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    }
  }, [])

  return {
    alerts,
    loading,
    error,
    unseenCount,
    hasMore,
    refresh,
    loadMore,
    markAllSeen,
    markOneSeen,
  }
}
