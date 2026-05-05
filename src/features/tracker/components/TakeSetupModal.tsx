import { useCallback, useEffect, useMemo, useRef, useState } from "react"
import { createPortal } from "react-dom"
import { AlertCircle, X } from "lucide-react"
import { Button } from "../../../shared/components/ui/button"
import { Alert, AlertDescription } from "../../../shared/components/ui/alert"
import {
  MAX_EQUITY_STALENESS_HOURS,
  orderTicketTakeSetup,
  type TicketReceipt,
} from "../../../shared/api/orderTicket"
import type { Setup } from "../types"
import { BracketSummary, GateAlerts, OverrideSection, computeRungs } from "./TakeSetupModalParts"

interface TakeSetupModalProps {
  open: boolean
  setup: Setup
  /** UTC ISO; if null or older than 24h, Send is hard-blocked. */
  equityFetchedAt?: string | null
  onClose: () => void
  onSubmitted?: (receipt: TicketReceipt) => void
}

const STALENESS_MS = MAX_EQUITY_STALENESS_HOURS * 60 * 60 * 1000

function isStale(fetchedAt: string | null | undefined): boolean {
  if (!fetchedAt) return true
  return Date.now() - new Date(fetchedAt).getTime() > STALENESS_MS
}

export function TakeSetupModal({
  open,
  setup,
  equityFetchedAt,
  onClose,
  onSubmitted,
}: TakeSetupModalProps) {
  const [showOverrides, setShowOverrides] = useState(false)
  const [overrideQty, setOverrideQty] = useState("")
  const [overrideStop, setOverrideStop] = useState("")
  const [overrideReason, setOverrideReason] = useState("")
  const [submitting, setSubmitting] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const previouslyFocusedRef = useRef<HTMLElement | null>(null)
  const cancelBtnRef = useRef<HTMLButtonElement>(null)

  useEffect(() => {
    if (!open) return
    previouslyFocusedRef.current = document.activeElement as HTMLElement | null
    setShowOverrides(false)
    setOverrideQty("")
    setOverrideStop("")
    setOverrideReason("")
    setError(null)
    setSubmitting(false)
    requestAnimationFrame(() => cancelBtnRef.current?.focus())
  }, [open])

  const handleClose = useCallback(() => {
    onClose()
    const prev = previouslyFocusedRef.current
    if (prev && typeof prev.focus === "function") prev.focus()
  }, [onClose])

  useEffect(() => {
    if (!open) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault()
        handleClose()
      }
    }
    document.addEventListener("keydown", onKey)
    return () => document.removeEventListener("keydown", onKey)
  }, [open, handleClose])

  const sizing = setup.sizing
  const ungated = !sizing
  const skipped = sizing?.skipped_reason ?? null
  const stale = isStale(equityFetchedAt)
  const rungs = useMemo(() => computeRungs(setup), [setup])

  const overrideQtyNum = overrideQty === "" ? null : Number(overrideQty)
  const overrideStopNum = overrideStop === "" ? null : Number(overrideStop)
  const overrideTouched = overrideQty !== "" || overrideStop !== ""
  const overrideReasonMissing = overrideTouched && overrideReason.trim() === ""

  const blocked = ungated || !!skipped || stale
  const canSend = !blocked && !overrideReasonMissing && !submitting

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    if (!canSend) return
    setSubmitting(true)
    setError(null)
    try {
      const receipt = await orderTicketTakeSetup({
        setupId: setup.id,
        overrideQty: overrideQtyNum,
        overrideStop: overrideStopNum,
        overrideReason: overrideTouched ? overrideReason.trim() : null,
      })
      onSubmitted?.(receipt)
      handleClose()
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setSubmitting(false)
    }
  }

  if (!open) return null

  return createPortal(
    <div
      role="dialog"
      aria-modal="true"
      aria-labelledby="take-setup-title"
      className="fixed inset-0 z-50 flex items-center justify-center p-4"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) handleClose()
      }}
    >
      <div className="absolute inset-0 bg-black/60" />
      <form
        onSubmit={handleSubmit}
        className="border-border bg-background text-foreground relative w-full max-w-md space-y-4 rounded-lg border p-5 shadow-xl"
      >
        <div className="flex items-start justify-between">
          <div>
            <h2 id="take-setup-title" className="text-lg font-semibold">
              Take Setup — {setup.symbol}
            </h2>
            <p className="text-muted-foreground text-xs capitalize">
              {setup.direction} · {setup.strategy}
            </p>
          </div>
          <Button
            type="button"
            variant="ghost"
            size="sm"
            onClick={handleClose}
            className="h-8 w-8 p-0"
            aria-label="Close"
          >
            <X className="h-4 w-4" />
          </Button>
        </div>

        <GateAlerts ungated={ungated} skipped={skipped} stale={stale} />

        {!blocked && sizing && <BracketSummary setup={setup} sizing={sizing} rungs={rungs} />}

        {!blocked && (
          <OverrideSection
            show={showOverrides}
            onToggle={() => setShowOverrides((s) => !s)}
            qty={overrideQty}
            stop={overrideStop}
            reason={overrideReason}
            onQty={setOverrideQty}
            onStop={setOverrideStop}
            onReason={setOverrideReason}
            reasonMissing={overrideReasonMissing}
            touched={overrideTouched}
          />
        )}

        {error && (
          <Alert variant="destructive">
            <AlertCircle className="h-4 w-4" />
            <AlertDescription>{error}</AlertDescription>
          </Alert>
        )}

        <div className="flex justify-end gap-2 pt-1">
          <Button
            ref={cancelBtnRef}
            type="button"
            variant="outline"
            onClick={handleClose}
            disabled={submitting}
          >
            Cancel
          </Button>
          <Button type="submit" disabled={!canSend} data-testid="take-setup-send">
            {submitting ? "Sending…" : "Send"}
          </Button>
        </div>
      </form>
    </div>,
    document.body,
  )
}
