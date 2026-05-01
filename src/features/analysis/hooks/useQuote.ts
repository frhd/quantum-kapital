import { useEffect, useRef, useState } from "react"
import { listen, type UnlistenFn } from "@tauri-apps/api/event"
import { ibkrApi } from "../../../shared/api/ibkr"
import type { DataTier, Quote } from "../../../shared/types"

export type QuoteError = "disconnected" | "no_permission" | "timeout" | "fetch_failed"

interface ConnectionStatusEvent {
  type: "ConnectionStatusChanged"
  data: { connected: boolean; message: string }
}

interface DataTierEvent {
  type: "DataTierDetected"
  data: { tier: DataTier }
}

// Tier-derived poll cadence. Real-time matches the prior 5s; delayed
// drops to 60s because the upstream tick is 15-min stale anyway, so
// faster polling is wasted RPC. `Unknown` returns null → suspended,
// which matches the disconnected behavior already implemented through
// `connectedRef`.
function pollIntervalForTier(tier: DataTier): number | null {
  switch (tier) {
    case "real_time":
      return 5_000
    case "delayed":
      return 60_000
    case "unknown":
      return null
  }
}

function classifyError(raw: unknown): QuoteError {
  const message = typeof raw === "string" ? raw : ((raw as Error)?.message ?? "")
  switch (message) {
    case "disconnected":
      return "disconnected"
    case "no_permission":
      return "no_permission"
    default:
      return "fetch_failed"
  }
}

export function useQuote(symbol: string | null) {
  const [quote, setQuote] = useState<Quote | null>(null)
  const [error, setError] = useState<QuoteError | null>(null)
  const [loading, setLoading] = useState(false)

  // Mutable polling state — kept in refs so the visibility / connection
  // listeners don't capture stale closures across re-renders.
  const intervalRef = useRef<number | null>(null)
  const connectedRef = useRef(true)
  const visibleRef = useRef(
    typeof document === "undefined" || document.visibilityState === "visible",
  )
  const symbolRef = useRef<string | null>(symbol)
  const tierRef = useRef<DataTier>("unknown")

  useEffect(() => {
    symbolRef.current = symbol
  }, [symbol])

  useEffect(() => {
    if (!symbol) {
      setQuote(null)
      setError(null)
      return
    }

    let cancelled = false

    const fetchOnce = async () => {
      const current = symbolRef.current
      if (!current) return
      setLoading(true)
      try {
        const result = await ibkrApi.getQuote(current)
        if (cancelled || symbolRef.current !== current) return
        setQuote(result)
        setError(null)
      } catch (err) {
        if (cancelled || symbolRef.current !== current) return
        setError(classifyError(err))
      } finally {
        if (!cancelled) setLoading(false)
      }
    }

    const stopInterval = () => {
      if (intervalRef.current !== null) {
        window.clearInterval(intervalRef.current)
        intervalRef.current = null
      }
    }

    // Recompute cadence from the current tier and (re)start the timer.
    // Always tears down the existing interval first because cadence
    // changes are rare but must take effect immediately (e.g. RealTime
    // → Delayed transitions on reconnect).
    const ensureRunning = () => {
      stopInterval()
      if (!connectedRef.current || !visibleRef.current) return
      const ms = pollIntervalForTier(tierRef.current)
      if (ms === null) return
      intervalRef.current = window.setInterval(fetchOnce, ms)
    }

    const onVisibility = () => {
      const wasVisible = visibleRef.current
      visibleRef.current = document.visibilityState === "visible"
      if (!wasVisible && visibleRef.current) {
        // Resumed — fetch immediately, then restart the timer.
        fetchOnce()
      }
      ensureRunning()
    }

    document.addEventListener("visibilitychange", onVisibility)

    let unlistenConnection: UnlistenFn | undefined
    let unlistenTier: UnlistenFn | undefined
    ;(async () => {
      try {
        unlistenConnection = await listen<ConnectionStatusEvent>(
          "connection-status-changed",
          (event) => {
            const wasConnected = connectedRef.current
            connectedRef.current = event.payload.data.connected
            if (!wasConnected && connectedRef.current) {
              fetchOnce()
            }
            if (!connectedRef.current) {
              setError("disconnected")
            }
            ensureRunning()
          },
        )
      } catch (err) {
        console.error("Failed to listen for connection-status-changed:", err)
      }
    })()
    ;(async () => {
      try {
        unlistenTier = await listen<DataTierEvent>("data-tier-detected", (event) => {
          const next = event.payload.data.tier
          if (tierRef.current === next) return
          tierRef.current = next
          ensureRunning()
        })
      } catch (err) {
        console.error("Failed to listen for data-tier-detected:", err)
      }
    })()

    // Hydrate tier on mount/symbol-change so a tab switch doesn't
    // wait for the next emission to start polling at the right cadence.
    ;(async () => {
      try {
        const tier = await ibkrApi.getDataTier()
        if (cancelled) return
        if (tierRef.current !== tier) {
          tierRef.current = tier
          ensureRunning()
        }
      } catch {
        // Pre-connect or transient — keep the default `unknown` tier
        // and wait for the next `data-tier-detected` event.
      }
    })()

    // Reset symbol-scoped state and kick off the first fetch.
    setQuote(null)
    setError(null)
    fetchOnce()
    ensureRunning()

    return () => {
      cancelled = true
      stopInterval()
      document.removeEventListener("visibilitychange", onVisibility)
      unlistenConnection?.()
      unlistenTier?.()
    }
  }, [symbol])

  return { quote, error, loading }
}
