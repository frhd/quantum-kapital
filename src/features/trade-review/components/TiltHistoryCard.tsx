import { useEffect, useState } from "react"
import { ShieldAlert } from "lucide-react"

import {
  TILT_RELEASE_LABELS,
  TILT_TRIGGER_LABELS,
  formatEtTime,
  tiltGuardHistory,
  type TiltEpisodeView,
} from "../../../shared/api/tiltGuard"
import { Card, CardContent, CardHeader, CardTitle } from "../../../shared/components/ui/card"
import { cn } from "../../../shared/lib/utils"

/**
 * Phase 11 — past tilt episodes for the trader-profile rollup.
 * Renders a compact list with trigger reason + release kind + reason
 * text. Master cross-phase verification calls out "if any single gate
 * exceeds 30% override rate over 60 days the gate is too strict OR
 * the trader is rationalizing"; the manual-override count surfaced
 * here is the data the trader-profile review reads against.
 */
export function TiltHistoryCard({ days = 60 }: { days?: number }) {
  const [rows, setRows] = useState<TiltEpisodeView[] | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let active = true
    tiltGuardHistory(days)
      .then((r) => {
        if (active) {
          setRows(r)
          setError(null)
        }
      })
      .catch((e) => active && setError(String(e)))
    return () => {
      active = false
    }
  }, [days])

  if (error) {
    return (
      <Card data-testid="tilt-history-error">
        <CardHeader>
          <CardTitle className="text-sm">Tilt episodes</CardTitle>
        </CardHeader>
        <CardContent className="text-muted-foreground text-xs">
          History unavailable: {error}
        </CardContent>
      </Card>
    )
  }

  if (!rows) return null

  const overrideCount = rows.filter((r) => r.release_kind === "manual_override").length
  const tripCount = rows.length

  return (
    <Card data-testid="tilt-history-card">
      <CardHeader className="pb-2">
        <div className="flex items-baseline justify-between gap-2">
          <CardTitle className="text-sm">Tilt episodes</CardTitle>
          <span className="text-muted-foreground text-[11px]">
            last {days}d · {tripCount} trip{tripCount === 1 ? "" : "s"}
            {overrideCount > 0
              ? ` · ${overrideCount} override${overrideCount === 1 ? "" : "s"}`
              : ""}
          </span>
        </div>
      </CardHeader>
      <CardContent>
        {tripCount === 0 ? (
          <p className="text-muted-foreground text-xs">No tilt episodes in the window.</p>
        ) : (
          <ul className="space-y-2">
            {rows.map((ep) => (
              <li
                key={ep.id}
                data-testid={`tilt-history-row-${ep.id}`}
                className="border-border/40 flex flex-col gap-1 rounded-md border px-3 py-2"
              >
                <div className="flex flex-wrap items-baseline gap-2 text-xs">
                  <ShieldAlert
                    className={cn(
                      "h-3 w-3",
                      ep.release_kind === "manual_override" ? "text-amber-300" : "text-rose-300",
                    )}
                    aria-hidden
                  />
                  <span className="font-medium">
                    {TILT_TRIGGER_LABELS[ep.trigger_kind] ?? ep.trigger_kind}
                  </span>
                  <span className="text-muted-foreground">cum {ep.cumulative_r.toFixed(2)}R</span>
                  {ep.consecutive_losses > 0 && (
                    <span className="text-muted-foreground">streak {ep.consecutive_losses}</span>
                  )}
                  <span className="text-muted-foreground/70 ml-auto">
                    {formatEtTime(ep.triggered_at)} ET
                  </span>
                </div>
                <div className="text-muted-foreground flex flex-wrap items-baseline gap-2 text-[11px]">
                  <span>
                    Released:{" "}
                    {ep.release_kind
                      ? (TILT_RELEASE_LABELS[ep.release_kind] ?? ep.release_kind)
                      : "open"}
                  </span>
                  {ep.released_at && <span>· {formatEtTime(ep.released_at)} ET</span>}
                  {ep.release_reason && (
                    <span className="text-foreground/70 italic">— {ep.release_reason}</span>
                  )}
                </div>
              </li>
            ))}
          </ul>
        )}
      </CardContent>
    </Card>
  )
}
