import { useEffect, useState } from "react"
import { CalendarClock, ShieldOff } from "lucide-react"
import {
  describeSkip,
  setupOverrideBlackout,
  trackerGetSkippedSetups,
} from "../../../shared/api/eventCalendar"
import { Button } from "../../../shared/components/ui/button"
import type { Setup } from "../types"

/**
 * Phase 5 — list of detector hits gated by an event blackout. The
 * trader can review each skip and override per-setup with a recorded
 * reason. Override produces a fresh non-skipped setup row + audit
 * entry; the panel re-fetches after a successful override.
 */
export function SkippedSetupsPanel({ since }: { since?: string | null }) {
  const [setups, setSetups] = useState<Setup[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  async function reload() {
    setLoading(true)
    try {
      const rows = await trackerGetSkippedSetups(since ?? null)
      setSetups(rows)
      setError(null)
    } catch (e) {
      setError(String(e))
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    void reload()
    // since is the only knob; reloading on change is intentional.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [since])

  if (loading) {
    return (
      <div className="text-muted-foreground p-3 text-sm" data-testid="skipped-setups-loading">
        Loading skipped setups…
      </div>
    )
  }
  if (error) {
    return (
      <div className="p-3 text-sm text-red-400" data-testid="skipped-setups-error">
        Failed to load skipped setups: {error}
      </div>
    )
  }
  if (setups.length === 0) {
    return (
      <div
        className="text-muted-foreground flex flex-col items-center gap-1 p-4 text-sm"
        data-testid="skipped-setups-empty"
      >
        <ShieldOff className="h-4 w-4 opacity-60" />
        <span>No setups gated by an event blackout.</span>
      </div>
    )
  }
  return (
    <div className="flex flex-col gap-2" data-testid="skipped-setups-panel">
      {setups.map((s) => (
        <SkippedSetupRow key={s.id} setup={s} onOverride={reload} />
      ))}
    </div>
  )
}

function SkippedSetupRow({ setup, onOverride }: { setup: Setup; onOverride: () => void }) {
  const [reason, setReason] = useState("")
  const [submitting, setSubmitting] = useState(false)
  const [submitError, setSubmitError] = useState<string | null>(null)

  const submitDisabled = !reason.trim() || submitting

  async function override() {
    setSubmitting(true)
    setSubmitError(null)
    try {
      await setupOverrideBlackout(setup.id, reason)
      onOverride()
    } catch (e) {
      setSubmitError(String(e))
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <div
      className="border-border/60 bg-card/40 flex flex-col gap-1.5 rounded-md border p-2"
      data-testid={`skipped-setup-row-${setup.id}`}
    >
      <div className="flex items-baseline justify-between gap-2">
        <div className="flex items-baseline gap-2">
          <span className="font-mono text-sm font-semibold">{setup.symbol}</span>
          <span className="text-muted-foreground text-[11px] tracking-wide uppercase">
            {setup.strategy} · {setup.direction}
          </span>
        </div>
        <span className="inline-flex items-center gap-1 text-[10px] tracking-wide text-amber-300 uppercase">
          <CalendarClock className="h-3 w-3" />
          {describeSkip(setup)}
        </span>
      </div>
      {setup.skip_window_json?.reason && (
        <p className="text-muted-foreground text-[11px]">{setup.skip_window_json.reason}</p>
      )}
      <div className="flex items-center gap-2 pt-1">
        <input
          type="text"
          value={reason}
          onChange={(e) => setReason(e.target.value)}
          placeholder="reason for override (required)"
          className="border-border bg-background flex-1 rounded-sm border px-2 py-1 text-xs"
          data-testid={`override-reason-${setup.id}`}
        />
        <Button
          type="button"
          size="sm"
          variant="outline"
          className="h-7 px-2 text-xs"
          disabled={submitDisabled}
          onClick={() => void override()}
          data-testid={`override-submit-${setup.id}`}
        >
          {submitting ? "Overriding…" : "Take anyway"}
        </Button>
      </div>
      {submitError && (
        <p className="text-[11px] text-red-400" data-testid={`override-error-${setup.id}`}>
          {submitError}
        </p>
      )}
    </div>
  )
}
