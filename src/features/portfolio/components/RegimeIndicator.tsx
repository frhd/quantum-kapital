import { useEffect, useState } from "react"
import { listen } from "@tauri-apps/api/event"
import { Activity, AlertTriangle } from "lucide-react"

import {
  BREADTH_LABELS,
  CORR_LABELS,
  TREND_LABELS,
  VOL_LABELS,
  regimeCurrent,
  type Regime,
  type RegimeChangedPayload,
  type RegimeCurrent,
} from "../../../shared/api/regime"
import { cn } from "../../../shared/lib/utils"

/**
 * Phase 9 — compact top-of-screen regime pill.
 * Subscribes to `regime-changed` so the indicator stays live without
 * polling. The pill shows the *stable* (post-3-day-persistence)
 * classification; tooltip on the icon surfaces missing-input warnings
 * and the raw read for diagnostics.
 */
export function RegimeIndicator() {
  const [data, setData] = useState<RegimeCurrent | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let active = true
    regimeCurrent()
      .then((r) => {
        if (active) {
          setData(r)
          setError(null)
        }
      })
      .catch((e) => active && setError(String(e)))
    return () => {
      active = false
    }
  }, [])

  useEffect(() => {
    const unlistenPromise = listen<RegimeChangedPayload>("regime-changed", () => {
      regimeCurrent()
        .then(setData)
        .catch((e) => setError(String(e)))
    })
    return () => {
      void unlistenPromise.then((u) => u())
    }
  }, [])

  if (error) {
    return (
      <div
        role="status"
        aria-label="Regime indicator unavailable"
        className="border-border/50 text-muted-foreground bg-card/30 inline-flex items-center gap-2 rounded-md border px-2 py-1 text-[11px]"
      >
        <AlertTriangle className="h-3 w-3 text-amber-400" aria-hidden />
        Regime: unavailable
      </div>
    )
  }

  if (!data) {
    return (
      <div
        role="status"
        className="border-border/50 text-muted-foreground bg-card/30 inline-flex items-center gap-2 rounded-md border px-2 py-1 text-[11px]"
      >
        <Activity className="h-3 w-3" aria-hidden />
        Regime: …
      </div>
    )
  }

  const stable = data.stable
  const stale = data.missing.length > 0
  const flipped =
    stable.trend !== data.raw.trend ||
    stable.vol !== data.raw.vol ||
    stable.breadth !== data.raw.breadth ||
    stable.corr !== data.raw.corr

  const tooltipParts: string[] = [
    `Source: ${data.source}`,
    `Raw: ${TREND_LABELS[data.raw.trend]} / ${VOL_LABELS[data.raw.vol]} / ${BREADTH_LABELS[data.raw.breadth]} / ${CORR_LABELS[data.raw.corr]}`,
  ]
  if (flipped) tooltipParts.push("Stable view differs from raw (3-day persistence rule active)")
  if (stale) tooltipParts.push(`Missing inputs: ${data.missing.join(", ")}`)

  return (
    <div
      role="status"
      aria-label="Current market regime"
      title={tooltipParts.join("\n")}
      className={cn(
        "border-border/50 inline-flex items-center gap-2 rounded-md border px-2 py-1 text-[11px]",
        stale ? "bg-amber-500/5 text-amber-100" : "bg-card/40 text-foreground",
      )}
    >
      {stale ? (
        <AlertTriangle className="h-3 w-3 text-amber-400" aria-hidden />
      ) : (
        <Activity className="h-3 w-3 text-emerald-400" aria-hidden />
      )}
      <RegimePart label="Trend" axis={TREND_LABELS[stable.trend]} flavor={trendFlavor(stable)} />
      <Divider />
      <RegimePart label="Vol" axis={VOL_LABELS[stable.vol]} flavor={volFlavor(stable)} />
      <Divider />
      <RegimePart
        label="Breadth"
        axis={BREADTH_LABELS[stable.breadth]}
        flavor={breadthFlavor(stable)}
      />
      <Divider />
      <RegimePart label="Corr" axis={CORR_LABELS[stable.corr]} flavor={corrFlavor(stable)} />
    </div>
  )
}

function Divider() {
  return <span className="text-muted-foreground/40">·</span>
}

function RegimePart({ label, axis, flavor }: { label: string; axis: string; flavor: string }) {
  return (
    <span className="inline-flex items-baseline gap-1">
      <span className="text-muted-foreground">{label}:</span>
      <span className={cn("font-medium", flavor)}>{axis}</span>
    </span>
  )
}

function trendFlavor(r: Regime): string {
  if (r.trend === "up") return "text-emerald-300"
  if (r.trend === "down") return "text-rose-300"
  return "text-zinc-300"
}

function volFlavor(r: Regime): string {
  if (r.vol === "high") return "text-rose-300"
  if (r.vol === "low") return "text-emerald-300"
  return "text-zinc-300"
}

function breadthFlavor(r: Regime): string {
  if (r.breadth === "healthy") return "text-emerald-300"
  if (r.breadth === "narrow") return "text-rose-300"
  return "text-zinc-300"
}

function corrFlavor(r: Regime): string {
  if (r.corr === "high") return "text-amber-300"
  if (r.corr === "low") return "text-emerald-300"
  return "text-zinc-300"
}
