import { useEffect, useState } from "react"
import { listen, type UnlistenFn } from "@tauri-apps/api/event"
import { ibkrApi } from "../../../shared/api/ibkr"
import type { ScannerData, ScannerSubscription } from "../../../shared/types"

interface ScannerEventPayload {
  type: "ScannerUpdate"
  data: { results: ScannerData[] }
}

export function useScanner(subscription: ScannerSubscription | null) {
  const [results, setResults] = useState<ScannerData[]>([])
  const [lastUpdate, setLastUpdate] = useState<Date | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (!subscription) {
      setResults([])
      setLastUpdate(null)
      setError(null)
      return
    }

    let unlisten: UnlistenFn | undefined
    let cancelled = false

    ;(async () => {
      try {
        unlisten = await listen<ScannerEventPayload>("scanner-update", (event) => {
          const payload = event.payload?.data
          if (payload) {
            setResults(payload.results)
            setLastUpdate(new Date())
          }
        })

        if (cancelled) {
          unlisten?.()
          return
        }

        await ibkrApi.startScanner(subscription)
      } catch (err) {
        const message = err instanceof Error ? err.message : String(err)
        console.error("Failed to start scanner:", message)
        setError(message)
      }
    })()

    return () => {
      cancelled = true
      unlisten?.()
      ibkrApi.stopScanner().catch((err) => {
        console.error("Failed to stop scanner:", err)
      })
    }
  }, [subscription])

  return { results, lastUpdate, error }
}
