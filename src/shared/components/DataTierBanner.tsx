import { useEffect, useState } from "react"
import { listen, type UnlistenFn } from "@tauri-apps/api/event"
import { AlertTriangle, Info } from "lucide-react"
import { ibkrApi } from "../api/ibkr"
import { cn } from "../lib/utils"
import type { DataTier } from "../types"

interface DataTierEvent {
  type: "DataTierDetected"
  data: { tier: DataTier }
}

interface ConnectionStatusEvent {
  type: "ConnectionStatusChanged"
  data: { connected: boolean; message: string }
}

interface BannerCopy {
  title: string
  description?: string
  variant: "info" | "warning"
}

function copyForTier(tier: DataTier): BannerCopy | null {
  switch (tier) {
    case "delayed":
      return {
        title: "Market data: Delayed (15-min)",
        description: "Some real-time-only features are unavailable.",
        variant: "warning",
      }
    case "unknown":
      return { title: "Detecting market data tier…", variant: "info" }
    case "real_time":
      return null
  }
}

export function DataTierBanner() {
  const [tier, setTier] = useState<DataTier>("unknown")
  const [connected, setConnected] = useState(false)

  useEffect(() => {
    let cancelled = false
    let unlistenTier: UnlistenFn | undefined
    let unlistenConn: UnlistenFn | undefined
    ;(async () => {
      try {
        unlistenTier = await listen<DataTierEvent>("data-tier-detected", (event) => {
          if (!cancelled) setTier(event.payload.data.tier)
        })
      } catch (err) {
        console.error("DataTierBanner: tier listener failed:", err)
      }
    })()
    ;(async () => {
      try {
        unlistenConn = await listen<ConnectionStatusEvent>("connection-status-changed", (event) => {
          if (!cancelled) setConnected(event.payload.data.connected)
        })
      } catch (err) {
        console.error("DataTierBanner: connection listener failed:", err)
      }
    })()

    // Hydrate so a fresh mount during an active connection doesn't
    // need to wait for the next event to render.
    ;(async () => {
      try {
        const status = await ibkrApi.getConnectionStatus()
        if (cancelled) return
        setConnected(Boolean(status?.connected))
      } catch {
        // Ignore — banner stays hidden until first event.
      }
      try {
        const current = await ibkrApi.getDataTier()
        if (!cancelled) setTier(current)
      } catch {
        // Ignore.
      }
    })()

    return () => {
      cancelled = true
      unlistenTier?.()
      unlistenConn?.()
    }
  }, [])

  if (!connected) return null
  const copy = copyForTier(tier)
  if (!copy) return null

  const Icon = copy.variant === "warning" ? AlertTriangle : Info
  return (
    <div
      role="status"
      data-testid="data-tier-banner"
      className={cn(
        "flex items-start gap-2 rounded-md border px-3 py-2 text-sm",
        copy.variant === "warning"
          ? "border-amber-700 bg-amber-900/40"
          : "border-border bg-card/95",
      )}
    >
      <Icon
        className={cn(
          "mt-0.5 h-4 w-4",
          copy.variant === "warning" ? "text-amber-300" : "text-blue-400",
        )}
      />
      <div className="flex-1">
        <p className="leading-tight font-medium">{copy.title}</p>
        {copy.description && <p className="text-foreground mt-0.5 text-xs">{copy.description}</p>}
      </div>
    </div>
  )
}
