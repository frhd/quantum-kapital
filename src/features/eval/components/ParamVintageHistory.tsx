import { useEffect, useState } from "react"

import { Loader2, History } from "lucide-react"

import { Card, CardContent, CardHeader, CardTitle } from "../../../shared/components/ui/card"
import {
  describeVintageWindow,
  isActiveVintage,
  paramRefitGetActive,
  paramRefitHistory,
  type ParamVintage,
} from "../../../shared/api/paramRefit"

const DETECTORS = ["breakout", "episodic_pivot", "parabolic_short"] as const
type Detector = (typeof DETECTORS)[number]

const DETECTOR_LABELS: Record<Detector, string> = {
  breakout: "Breakout",
  episodic_pivot: "Episodic pivot",
  parabolic_short: "Parabolic short",
}

/**
 * Phase 10 — per-detector vintage timeline. Renders the active
 * vintage's params + the most recent N historical vintages so the
 * operator can see what changed from one refit to the next, and
 * judge whether the lock-on-improvement (10% beat) guard is
 * actually preventing churn.
 */
export function ParamVintageHistory() {
  const [detector, setDetector] = useState<Detector>("breakout")
  const [active, setActive] = useState<ParamVintage[] | null>(null)
  const [history, setHistory] = useState<ParamVintage[] | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    setLoading(true)
    setError(null)
    Promise.all([paramRefitGetActive(), paramRefitHistory(detector, 20)])
      .then(([a, h]) => {
        if (cancelled) return
        setActive(a)
        setHistory(h)
      })
      .catch((e: unknown) => {
        if (cancelled) return
        setError(typeof e === "string" ? e : ((e as Error)?.message ?? "load failed"))
      })
      .finally(() => {
        if (!cancelled) setLoading(false)
      })
    return () => {
      cancelled = true
    }
  }, [detector])

  const activeForThis = active?.find((v) => v.detector === detector) ?? null

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <History className="h-5 w-5" />
          Param vintages
        </CardTitle>
      </CardHeader>
      <CardContent className="space-y-4">
        <div className="flex items-center gap-2">
          <span className="text-muted-foreground text-sm">Detector:</span>
          {DETECTORS.map((d) => (
            <button
              key={d}
              onClick={() => setDetector(d)}
              className={`rounded border px-2 py-1 text-xs ${
                detector === d
                  ? "border-foreground bg-muted"
                  : "border-border text-muted-foreground hover:bg-muted/50"
              }`}
            >
              {DETECTOR_LABELS[d]}
            </button>
          ))}
          {loading && <Loader2 className="text-muted-foreground h-4 w-4 animate-spin" />}
        </div>

        {error && <p className="text-destructive text-sm">{error}</p>}

        {activeForThis ? (
          <ActiveVintageCard vintage={activeForThis} />
        ) : (
          <p className="text-muted-foreground text-sm">
            No active vintage for {DETECTOR_LABELS[detector]} — runner uses settings.toml bounds.
            The startup backfill should populate one on the next refit.
          </p>
        )}

        {history && history.length > 0 && (
          <div className="space-y-2">
            <h4 className="text-muted-foreground text-xs font-medium uppercase">History</h4>
            <ul className="space-y-1">
              {history.map((v) => (
                <li
                  key={v.vintage_id}
                  className="border-border bg-muted/40 rounded border px-3 py-2 text-xs"
                >
                  <div className="flex items-center justify-between">
                    <span className="font-mono">{v.vintage_id}</span>
                    <span
                      className={isActiveVintage(v) ? "text-emerald-500" : "text-muted-foreground"}
                    >
                      {isActiveVintage(v) ? "active" : "superseded"}
                    </span>
                  </div>
                  <div className="text-muted-foreground mt-1">
                    objective={v.objective_value.toFixed(3)}, n_oos={v.oos_n_trades}, source=
                    {v.source}
                  </div>
                  <div className="text-muted-foreground">{describeVintageWindow(v)}</div>
                </li>
              ))}
            </ul>
          </div>
        )}
      </CardContent>
    </Card>
  )
}

function ActiveVintageCard({ vintage }: { vintage: ParamVintage }) {
  return (
    <div className="border-border bg-card rounded border px-3 py-3">
      <div className="text-foreground flex items-center justify-between text-sm font-medium">
        <span>Active</span>
        <span className="font-mono text-xs">{vintage.vintage_id}</span>
      </div>
      <dl className="text-muted-foreground mt-2 grid grid-cols-2 gap-x-4 gap-y-1 text-xs">
        <dt>Objective (OOS PF)</dt>
        <dd className="text-foreground">{vintage.objective_value.toFixed(3)}</dd>
        <dt>OOS trades</dt>
        <dd className="text-foreground">{vintage.oos_n_trades}</dd>
        <dt>Source</dt>
        <dd className="text-foreground">{vintage.source}</dd>
        <dt>Locked</dt>
        <dd className="text-foreground">{vintage.locked_at}</dd>
      </dl>
      <div className="text-muted-foreground mt-2 text-xs">{describeVintageWindow(vintage)}</div>
      <details className="mt-2 text-xs">
        <summary className="text-muted-foreground cursor-pointer">params</summary>
        <pre className="bg-muted/60 text-foreground mt-1 overflow-auto rounded px-2 py-2 text-[11px]">
          {JSON.stringify(vintage.params_json, null, 2)}
        </pre>
      </details>
      {vintage.notes && (
        <p className="text-muted-foreground mt-2 text-xs italic">notes: {vintage.notes}</p>
      )}
    </div>
  )
}
