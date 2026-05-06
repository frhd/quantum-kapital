import type { ExitPlan } from "../../../shared/api/exits"

interface ExitPlanCardProps {
  plan: ExitPlan | null
  /** Optional: show a loading state when the modal is fetching the
   *  policy preview. */
  loading?: boolean
}

// Phase 7 — render the persisted exit policy. Used by TakeSetupModal
// so the trader sees exactly which ladder + trail + time-stop will be
// attached *before* confirming. Falls back to a neutral "static
// policy" message when no plan is wired (pre-P7 setups, ATR
// unavailable, etc).
export function ExitPlanCard({ plan, loading }: ExitPlanCardProps) {
  if (loading) {
    return (
      <div className="border-border rounded border p-3 text-sm">
        <div className="text-muted-foreground">Loading exit plan…</div>
      </div>
    )
  }
  if (!plan) {
    return (
      <div className="border-border rounded border p-3 text-sm">
        <div className="font-medium">Exit plan</div>
        <div className="text-muted-foreground mt-1">
          Static 50/30/20 at 1R/2R/3R (default fallback). No trail or time-stop attached.
        </div>
      </div>
    )
  }
  const isAtr = plan.policy_version === "v2_atr_scaled"
  return (
    <div data-testid="exit-plan-card" className="border-border rounded border p-3 text-sm">
      <div className="flex items-center justify-between">
        <div className="font-medium">Exit plan</div>
        <span
          className="bg-muted text-muted-foreground rounded px-2 py-0.5 font-mono text-xs"
          data-testid="exit-plan-policy"
        >
          {plan.policy_version}
        </span>
      </div>

      <ul className="mt-2 space-y-0.5">
        {plan.targets.map((t) => (
          <li key={t.label} className="flex justify-between font-mono text-xs">
            <span className="text-muted-foreground">
              {t.qty_pct}% @ {t.label}
            </span>
            <span>${t.price.toFixed(2)}</span>
          </li>
        ))}
      </ul>

      {plan.trail && (
        <div className="text-muted-foreground mt-2 text-xs">
          Trail: chandelier × {plan.trail.atr_multiple}×ATR
          {plan.trail.activate_after_label
            ? ` (activates after ${plan.trail.activate_after_label})`
            : ""}
          {plan.trail.move_to_break_even_at_r
            ? ` · BE @ ${plan.trail.move_to_break_even_at_r}R`
            : ""}
        </div>
      )}
      {plan.time_stop && (
        <div className="text-muted-foreground mt-1 text-xs">
          Time stop: {plan.time_stop.max_trading_days} trading days
        </div>
      )}
      {!plan.trail && !plan.time_stop && (
        <div className="text-muted-foreground mt-1 text-xs">
          No trail or time-stop (legacy static policy).
        </div>
      )}
      {isAtr && plan.atr_at_signal && (
        <div className="text-muted-foreground mt-1 text-[10px]">
          ATR(20) at signal: ${plan.atr_at_signal.toFixed(3)}
        </div>
      )}
    </div>
  )
}
