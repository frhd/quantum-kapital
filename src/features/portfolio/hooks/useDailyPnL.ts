import { useEffect, useState } from "react"
import { listen, type UnlistenFn } from "@tauri-apps/api/event"
import { ibkrApi } from "../../../shared/api/ibkr"
import type { DailyPnL } from "../../../shared/types"

interface DailyPnLEventPayload {
  type: "DailyPnLUpdate"
  data: DailyPnL
}

export function useDailyPnL(account: string | undefined) {
  const [dailyPnL, setDailyPnL] = useState<DailyPnL | null>(null)

  useEffect(() => {
    if (!account) {
      setDailyPnL(null)
      return
    }

    let unlisten: UnlistenFn | undefined
    let cancelled = false

    ;(async () => {
      try {
        unlisten = await listen<DailyPnLEventPayload>("daily-pnl-update", (event) => {
          const payload = event.payload?.data
          if (payload && payload.account === account) {
            setDailyPnL(payload)
          }
        })

        if (cancelled) {
          unlisten?.()
          return
        }

        await ibkrApi.startDailyPnL(account)
      } catch (err) {
        console.error("Failed to start daily PnL subscription:", err)
      }
    })()

    return () => {
      cancelled = true
      unlisten?.()
      ibkrApi.stopDailyPnL().catch((err) => {
        console.error("Failed to stop daily PnL subscription:", err)
      })
    }
  }, [account])

  return dailyPnL
}
