import { useState } from "react"
import { AlertTriangle } from "lucide-react"
import {
  formatDollarRisk,
  formatEquity,
  formatRPerShare,
  SIZING_SKIPPED_LABELS,
  type Sizing,
} from "../../../shared/api/riskEngine"
import { Button } from "../../../shared/components/ui/button"
import { cn } from "../../../shared/lib/utils"
import type { Setup } from "../types"
import { SetupBadge } from "./SetupBadge"
import { TakeSetupModal } from "./TakeSetupModal"

interface SetupCardProps {
  setup: Setup
  /**
   * Last fetched timestamp of the equity snapshot the sizing pinned
   * to (UTC ISO). When provided and older than ~1 trading day, the
   * card surfaces a "stale equity" warning. When undefined, the
   * staleness check is skipped — the equity_at_decision_cents alone
   * isn't enough to know the snapshot age.
   */
  equityFetchedAt?: string | null
}

const ONE_TRADING_DAY_MS = 24 * 60 * 60 * 1000

const GRADE_CLASSES: Record<"A" | "B" | "C", string> = {
  A: "bg-emerald-500/15 text-emerald-300 border-emerald-500/40",
  B: "bg-amber-500/15 text-amber-300 border-amber-500/40",
  C: "bg-secondary text-foreground/80 border-border",
}

function isStale(fetchedAt: string | null | undefined): boolean {
  if (!fetchedAt) return false
  const age = Date.now() - new Date(fetchedAt).getTime()
  return age > ONE_TRADING_DAY_MS
}

function SizingRow({ sizing }: { sizing: Sizing }) {
  if (sizing.skipped_reason) {
    return (
      <div
        className="text-foreground/70 inline-flex items-center gap-1 text-[11px]"
        data-testid="sizing-skipped"
      >
        <AlertTriangle className="h-3 w-3 text-amber-400" />
        <span>Skipped — {SIZING_SKIPPED_LABELS[sizing.skipped_reason]}</span>
      </div>
    )
  }
  return (
    <div className="text-foreground/80 flex flex-wrap items-center gap-x-3 gap-y-1 text-[11px]">
      <span
        className={cn(
          "rounded-sm border px-1.5 py-0.5 font-mono text-[10px] font-semibold",
          GRADE_CLASSES[sizing.conviction_grade],
        )}
      >
        {sizing.conviction_grade}
      </span>
      <span data-testid="sizing-qty">
        <span className="text-muted-foreground mr-1 tracking-wide uppercase">qty</span>
        <span className="font-mono">{sizing.qty}</span>
      </span>
      <span data-testid="sizing-dollar-risk">
        <span className="text-muted-foreground mr-1 tracking-wide uppercase">risk</span>
        <span className="font-mono">{formatDollarRisk(sizing.dollar_risk_cents)}</span>
      </span>
      <span data-testid="sizing-r-per-share">
        <span className="text-muted-foreground mr-1 tracking-wide uppercase">R/sh</span>
        <span className="font-mono">{formatRPerShare(sizing.r_per_share_cents)}</span>
      </span>
      <span className="text-muted-foreground" data-testid="sizing-equity">
        @ {formatEquity(sizing.equity_at_decision_cents)}
        {sizing.cap_applied ? " · capped" : ""}
      </span>
    </div>
  )
}

/**
 * Tracker setup card. Surfaces the strategy badge alongside the
 * Phase 1 risk-engine sizing fields. When `sizing` is `null` (pre-P1
 * rows or the sized-blind back-compat path), renders an "ungated"
 * pill so the trader knows the engine didn't touch the row.
 */
export function SetupCard({ setup, equityFetchedAt }: SetupCardProps) {
  const [modalOpen, setModalOpen] = useState(false)
  const stale = isStale(equityFetchedAt)
  const sizingMissing = !setup.sizing || !!setup.sizing.skipped_reason
  const takeDisabled = sizingMissing || stale
  return (
    <div
      className="border-border bg-card/50 flex flex-col gap-1.5 rounded-md border p-2"
      data-testid="setup-card"
    >
      <div className="flex items-center justify-between gap-2">
        <SetupBadge setup={setup} />
        {stale && (
          <span
            className="inline-flex items-center gap-1 text-[10px] tracking-wide text-amber-400 uppercase"
            title={`Equity snapshot fetched ${equityFetchedAt} — older than 1 trading day`}
            data-testid="ungated-equity-warning"
          >
            <AlertTriangle className="h-3 w-3" />
            stale equity
          </span>
        )}
      </div>
      {setup.sizing ? (
        <SizingRow sizing={setup.sizing} />
      ) : (
        <span className="text-muted-foreground text-[11px] italic" data-testid="sizing-ungated">
          ungated — sizing not run
        </span>
      )}
      <div className="flex justify-end">
        <Button
          type="button"
          size="sm"
          variant="outline"
          className="h-7 px-2 text-xs"
          disabled={takeDisabled}
          onClick={() => setModalOpen(true)}
          data-testid="take-setup-button"
        >
          Take Setup
        </Button>
      </div>
      <TakeSetupModal
        open={modalOpen}
        setup={setup}
        equityFetchedAt={equityFetchedAt}
        onClose={() => setModalOpen(false)}
      />
    </div>
  )
}
