import { useEffect, useRef, useState } from "react"
import { listen, type UnlistenFn } from "@tauri-apps/api/event"
import { ibkrApi } from "../../../shared/api/ibkr"
import type { Quote } from "../../../shared/types"

export type QuoteError =
  | "disconnected"
  | "no_permission"
  | "timeout"
  | "fetch_failed"

interface ConnectionStatusEvent {
  type: "ConnectionStatusChanged"
  data: { connected: boolean; message: string }
}

const POLL_MS = 5_000

function classifyError(raw: unknown): QuoteError {
  const message = typeof raw === "string" ? raw : (raw as Error)?.message ?? ""
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

    const startInterval = () => {
      if (intervalRef.current !== null) return
      intervalRef.current = window.setInterval(fetchOnce, POLL_MS)
    }

    const stopInterval = () => {
      if (intervalRef.current !== null) {
        window.clearInterval(intervalRef.current)
        intervalRef.current = null
      }
    }

    const ensureRunning = () => {
      if (connectedRef.current && visibleRef.current) {
        startInterval()
      } else {
        stopInterval()
      }
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
    }
  }, [symbol])

  return { quote, error, loading }
}
