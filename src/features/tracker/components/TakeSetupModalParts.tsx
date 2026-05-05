import { AlertCircle, ChevronDown, ChevronRight } from "lucide-react"
import { Input } from "../../../shared/components/ui/input"
import { Label } from "../../../shared/components/ui/label"
import { Alert, AlertDescription } from "../../../shared/components/ui/alert"
import {
  formatDollarRisk,
  formatRPerShare,
  SIZING_SKIPPED_LABELS,
  type Sizing,
  type SizingSkippedReason,
} from "../../../shared/api/riskEngine"
import { STATIC_TARGET_LADDER } from "../../../shared/api/orderTicket"
import type { Setup } from "../types"

interface GateAlertsProps {
  ungated: boolean
  skipped: SizingSkippedReason | null
  stale: boolean
}

export function GateAlerts({ ungated, skipped, stale }: GateAlertsProps) {
  if (ungated) {
    return (
      <Alert variant="destructive" data-testid="gate-ungated">
        <AlertCircle className="h-4 w-4" />
        <AlertDescription>ungated — no sizing</AlertDescription>
      </Alert>
    )
  }
  if (skipped) {
    return (
      <Alert variant="destructive" data-testid="gate-skipped">
        <AlertCircle className="h-4 w-4" />
        <AlertDescription>sizing skipped: {SIZING_SKIPPED_LABELS[skipped]}</AlertDescription>
      </Alert>
    )
  }
  if (stale) {
    return (
      <Alert variant="destructive" data-testid="gate-stale">
        <AlertCircle className="h-4 w-4" />
        <AlertDescription>stale equity — refresh before sending</AlertDescription>
      </Alert>
    )
  }
  return null
}

export interface Rung {
  label: string
  pct: number
  price: number
}

export function computeRungs(setup: Setup): Rung[] {
  const r = Math.abs(setup.trigger_price - setup.stop_price)
  const sign = setup.direction === "long" ? 1 : -1
  return STATIC_TARGET_LADDER.map(({ label, pct, rMultiple }) => ({
    label,
    pct,
    price: setup.trigger_price + sign * rMultiple * r,
  }))
}

const formatPrice = (p: number) => `$${p.toFixed(2)}`

interface BracketSummaryProps {
  setup: Setup
  sizing: Sizing
  rungs: Rung[]
}

export function BracketSummary({ setup, sizing, rungs }: BracketSummaryProps) {
  return (
    <div className="space-y-3" data-testid="bracket-summary">
      <table className="w-full text-xs">
        <tbody>
          <tr>
            <td className="text-muted-foreground py-0.5 pr-2">Qty / Grade</td>
            <td className="font-mono" data-testid="summary-qty">
              {sizing.qty} · {sizing.conviction_grade}
            </td>
          </tr>
          <tr>
            <td className="text-muted-foreground py-0.5 pr-2">$ Risk · R/sh</td>
            <td className="font-mono">
              {formatDollarRisk(sizing.dollar_risk_cents)} ·{" "}
              {formatRPerShare(sizing.r_per_share_cents)}
            </td>
          </tr>
          <tr>
            <td className="text-muted-foreground py-0.5 pr-2">Trigger · Stop</td>
            <td className="font-mono">
              {formatPrice(setup.trigger_price)} · {formatPrice(setup.stop_price)}
            </td>
          </tr>
        </tbody>
      </table>
      <div className="border-border space-y-1 rounded border p-2">
        <p className="text-muted-foreground text-[10px] tracking-wide uppercase">
          Targets (50 / 30 / 20)
        </p>
        {rungs.map((r) => (
          <div
            key={r.label}
            className="flex justify-between font-mono text-xs"
            data-testid={`rung-${r.label}`}
          >
            <span>
              {r.label} · {r.pct}%
            </span>
            <span>{formatPrice(r.price)}</span>
          </div>
        ))}
      </div>
    </div>
  )
}

interface OverrideSectionProps {
  show: boolean
  onToggle: () => void
  qty: string
  stop: string
  reason: string
  onQty: (v: string) => void
  onStop: (v: string) => void
  onReason: (v: string) => void
  reasonMissing: boolean
  touched: boolean
}

export function OverrideSection({
  show,
  onToggle,
  qty,
  stop,
  reason,
  onQty,
  onStop,
  onReason,
  reasonMissing,
  touched,
}: OverrideSectionProps) {
  return (
    <div className="space-y-2">
      <button
        type="button"
        onClick={onToggle}
        className="text-muted-foreground hover:text-foreground inline-flex items-center gap-1 text-xs"
      >
        {show ? <ChevronDown className="h-3 w-3" /> : <ChevronRight className="h-3 w-3" />}
        Override qty / stop
      </button>
      {show && (
        <div className="space-y-2" data-testid="override-form">
          <div className="grid grid-cols-2 gap-2">
            <div className="space-y-1">
              <Label htmlFor="override-qty" className="text-xs">
                Qty
              </Label>
              <Input
                id="override-qty"
                type="number"
                min={1}
                value={qty}
                onChange={(e) => onQty(e.target.value)}
                className="bg-card h-8 text-xs"
              />
            </div>
            <div className="space-y-1">
              <Label htmlFor="override-stop" className="text-xs">
                Stop
              </Label>
              <Input
                id="override-stop"
                type="number"
                step="0.01"
                value={stop}
                onChange={(e) => onStop(e.target.value)}
                className="bg-card h-8 text-xs"
              />
            </div>
          </div>
          <div className="space-y-1">
            <Label htmlFor="override-reason" className="text-xs">
              Reason {touched && <span className="text-destructive">*</span>}
            </Label>
            <Input
              id="override-reason"
              value={reason}
              onChange={(e) => onReason(e.target.value)}
              placeholder="Why are you overriding?"
              className="bg-card h-8 text-xs"
            />
          </div>
          {reasonMissing && (
            <p className="text-destructive text-xs" data-testid="override-reason-error">
              Reason required when qty or stop is overridden.
            </p>
          )}
        </div>
      )}
    </div>
  )
}
