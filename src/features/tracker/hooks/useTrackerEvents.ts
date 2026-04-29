import { useEffect, useRef, useState } from "react"
import { listen, type UnlistenFn } from "@tauri-apps/api/event"
import type {
  Setup,
  SetupDetectedPayload,
  SetupInvalidatedPayload,
  TickerStatusChangedPayload,
  TrackerEvent,
} from "../types"

interface TauriEnvelope<TType extends string, TData> {
  type: TType
  data: TData
}

type SetupDetectedEnvelope = TauriEnvelope<"SetupDetected", SetupDetectedPayload>
type SetupInvalidatedEnvelope = TauriEnvelope<"SetupInvalidated", SetupInvalidatedPayload>
type TickerStatusChangedEnvelope = TauriEnvelope<
  "TickerStatusChanged",
  TickerStatusChangedPayload
>

const MAX_EVENTS = 100

export interface UseTrackerEventsResult {
  recentEvents: TrackerEvent[]
  lastSetupDetected: SetupDetectedPayload | null
  lastInvalidated: SetupInvalidatedPayload | null
  lastStatusChanged: TickerStatusChangedPayload | null
  /// Per-symbol latest active setup. Cleared when a setup-invalidated
  /// event arrives for the same symbol. Watchlist rows read this map
  /// to render the SetupBadge without an extra fetch.
  activeSetupBySymbol: Record<string, Setup>
}

export function useTrackerEvents(): UseTrackerEventsResult {
  const [recentEvents, setRecentEvents] = useState<TrackerEvent[]>([])
  const [lastSetupDetected, setLastSetupDetected] = useState<SetupDetectedPayload | null>(null)
  const [lastInvalidated, setLastInvalidated] = useState<SetupInvalidatedPayload | null>(null)
  const [lastStatusChanged, setLastStatusChanged] =
    useState<TickerStatusChangedPayload | null>(null)
  const [activeSetupBySymbol, setActiveSetupBySymbol] = useState<Record<string, Setup>>({})
  const cancelledRef = useRef(false)

  useEffect(() => {
    cancelledRef.current = false
    const unlisteners: UnlistenFn[] = []

    const pushEvent = (ev: TrackerEvent) => {
      setRecentEvents((prev) => {
        const next = [...prev, ev]
        return next.length > MAX_EVENTS ? next.slice(next.length - MAX_EVENTS) : next
      })
    }

    ;(async () => {
      try {
        const u1 = await listen<SetupDetectedEnvelope>("setup-detected", (event) => {
          const payload = event.payload?.data
          if (!payload) return
          setLastSetupDetected(payload)
          setActiveSetupBySymbol((prev) => ({ ...prev, [payload.setup.symbol]: payload.setup }))
          pushEvent({ kind: "setup-detected", payload })
        })
        if (cancelledRef.current) {
          u1()
          return
        }
        unlisteners.push(u1)

        const u2 = await listen<SetupInvalidatedEnvelope>("setup-invalidated", (event) => {
          const payload = event.payload?.data
          if (!payload) return
          setLastInvalidated(payload)
          setActiveSetupBySymbol((prev) => {
            if (!(payload.symbol in prev)) return prev
            const next = { ...prev }
            delete next[payload.symbol]
            return next
          })
          pushEvent({ kind: "setup-invalidated", payload })
        })
        if (cancelledRef.current) {
          u2()
          return
        }
        unlisteners.push(u2)

        const u3 = await listen<TickerStatusChangedEnvelope>(
          "ticker-status-changed",
          (event) => {
            const payload = event.payload?.data
            if (!payload) return
            setLastStatusChanged(payload)
            pushEvent({ kind: "ticker-status-changed", payload })
          },
        )
        if (cancelledRef.current) {
          u3()
          return
        }
        unlisteners.push(u3)
      } catch (err) {
        console.error("useTrackerEvents: listen failed", err)
      }
    })()

    return () => {
      cancelledRef.current = true
      for (const fn of unlisteners) {
        try {
          fn()
        } catch (err) {
          console.error("useTrackerEvents: unlisten failed", err)
        }
      }
    }
  }, [])

  return {
    recentEvents,
    lastSetupDetected,
    lastInvalidated,
    lastStatusChanged,
    activeSetupBySymbol,
  }
}
