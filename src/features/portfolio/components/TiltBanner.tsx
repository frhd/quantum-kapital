import { useEffect, useState } from "react"
import { listen } from "@tauri-apps/api/event"
import { AlertTriangle, ShieldAlert } from "lucide-react"

import {
  TILT_RELEASE_LABELS,
  TILT_TRIGGER_LABELS,
  formatEtTime,
  formatRelativeReset,
  tiltGuardOverride,
  tiltGuardStatus,
  type TiltActivatedPayload,
  type TiltReleasedPayload,
  type TiltStatus,
} from "../../../shared/api/tiltGuard"
import { Button } from "../../../shared/components/ui/button"
import { cn } from "../../../shared/lib/utils"

/**
 * Phase 11 — persistent tilt-state banner. Renders red while paused,
 * a soft "released" pill for the most recent episode after release, and
 * disappears when no recent tilt history exists. Dismiss surfaces a
 * reason input that posts through `tiltGuardOverride`; the override is
 * audited in `gate_overrides` server-side.
 */
export function TiltBanner() {
  const [status, setStatus] = useState<TiltStatus | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [overrideOpen, setOverrideOpen] = useState(false)
  const [reason, setReason] = useState("")
  const [submitting, setSubmitting] = useState(false)

  useEffect(() => {
    let active = true
    tiltGuardStatus()
      .then((s) => {
        if (active) {
          setStatus(s)
          setError(null)
        }
      })
      .catch((e) => active && setError(String(e)))
    return () => {
      active = false
    }
  }, [])

  useEffect(() => {
    const refresh = () => {
      tiltGuardStatus()
        .then(setStatus)
        .catch((e) => setError(String(e)))
    }
    const onActivated = listen<TiltActivatedPayload>("tilt-activated", refresh)
    const onReleased = listen<TiltReleasedPayload>("tilt-released", refresh)
    return () => {
      void onActivated.then((u) => u())
      void onReleased.then((u) => u())
    }
  }, [])

  if (error) {
    return (
      <div
        role="alert"
        data-testid="tilt-banner-error"
        className="border-border/50 text-muted-foreground bg-card/30 inline-flex items-center gap-2 rounded-md border px-3 py-2 text-xs"
      >
        <AlertTriangle className="h-3 w-3 text-amber-400" aria-hidden />
        Tilt status unavailable
      </div>
    )
  }

  if (!status) return null

  // Hide entirely when no current pause and no recent episode history —
  // no point showing a green "you're fine" pill on every screen.
  if (!status.paused && !status.episode) return null

  const handleSubmit = async () => {
    if (!reason.trim()) return
    setSubmitting(true)
    try {
      const next = await tiltGuardOverride(reason)
      setStatus(next)
      setReason("")
      setOverrideOpen(false)
    } catch (e) {
      setError(String(e))
    } finally {
      setSubmitting(false)
    }
  }

  if (status.paused && status.episode) {
    const ep = status.episode
    const triggerLabel = TILT_TRIGGER_LABELS[ep.trigger_kind] ?? ep.trigger_kind
    return (
      <div
        role="alert"
        data-testid="tilt-banner-paused"
        className={cn("rounded-md border border-rose-500/60 bg-rose-500/10 px-3 py-2 text-xs")}
      >
        <div className="flex flex-wrap items-center gap-3">
          <ShieldAlert className="h-4 w-4 text-rose-300" aria-hidden />
          <div className="flex flex-1 flex-wrap items-baseline gap-x-4 gap-y-1">
            <span className="font-medium text-rose-100">Tilt-paused</span>
            <span className="text-rose-100/80">
              {triggerLabel} · cum {ep.cumulative_r.toFixed(2)}R
              {ep.consecutive_losses > 0 ? ` · streak ${ep.consecutive_losses}` : ""}
            </span>
            <span className="text-rose-100/60">
              auto-resume {formatRelativeReset(ep.auto_reset_at)} ({formatEtTime(ep.auto_reset_at)}{" "}
              ET)
            </span>
          </div>
          {!overrideOpen && (
            <Button
              size="sm"
              variant="ghost"
              className="text-rose-200 hover:bg-rose-500/20"
              onClick={() => setOverrideOpen(true)}
              data-testid="tilt-banner-override-open"
            >
              Override
            </Button>
          )}
        </div>
        {overrideOpen && (
          <div className="mt-2 flex flex-col gap-2">
            <label htmlFor="tilt-override-reason" className="text-rose-100/80">
              Reason (logged for trader-profile review):
            </label>
            <textarea
              id="tilt-override-reason"
              data-testid="tilt-override-reason"
              className="border-border/40 bg-card/40 text-foreground rounded-md border px-2 py-1 text-xs"
              value={reason}
              onChange={(e) => setReason(e.target.value)}
              rows={2}
              disabled={submitting}
            />
            <div className="flex justify-end gap-2">
              <Button
                size="sm"
                variant="ghost"
                onClick={() => {
                  setOverrideOpen(false)
                  setReason("")
                }}
                disabled={submitting}
              >
                Cancel
              </Button>
              <Button
                size="sm"
                onClick={handleSubmit}
                disabled={!reason.trim() || submitting}
                data-testid="tilt-override-submit"
              >
                {submitting ? "Releasing…" : "Release tilt"}
              </Button>
            </div>
          </div>
        )}
      </div>
    )
  }

  // Released — show a soft pill so the trader sees that the gate fired
  // and is now off. Hidden once history grows past 7d (filtered server-side).
  const ep = status.episode
  if (!ep) return null
  const releaseLabel = ep.release_kind
    ? (TILT_RELEASE_LABELS[ep.release_kind] ?? ep.release_kind)
    : "—"
  return (
    <div
      role="status"
      data-testid="tilt-banner-released"
      className="border-border/40 text-muted-foreground bg-card/30 inline-flex items-center gap-2 rounded-md border px-3 py-1.5 text-[11px]"
    >
      <ShieldAlert className="h-3 w-3 text-amber-300" aria-hidden />
      <span>Tilt released ({releaseLabel})</span>
      {ep.release_reason && (
        <span className="text-muted-foreground/70 italic">— {ep.release_reason}</span>
      )}
    </div>
  )
}
