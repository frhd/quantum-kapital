import { AlertTriangle, ShieldAlert } from "lucide-react"

import { formatCents, GATE_KIND_LABELS, type GateResult } from "../../../shared/api/portfolioRisk"
import { cn } from "../../../shared/lib/utils"

/**
 * Phase 8 — banner shown inside `TakeSetupModal` when the
 * concentration gate fires `warn` or `block`. `warn` lets the trader
 * proceed (no override required); `block` requires a non-empty reason
 * captured upstream and routed through `concentrationRecordOverride`.
 */
export function GateWarningBanner({ result }: { result: GateResult }) {
  if (result.severity === "pass") return null

  const isBlock = result.severity === "block"
  return (
    <div
      role="alert"
      className={cn(
        "rounded-md border px-3 py-2 text-xs",
        isBlock
          ? "border-rose-500/50 bg-rose-500/10 text-rose-100"
          : "border-amber-500/50 bg-amber-500/10 text-amber-100",
      )}
      data-testid="gate-warning-banner"
    >
      <div className="mb-1 flex items-center gap-2 font-semibold">
        {isBlock ? <ShieldAlert className="h-4 w-4" /> : <AlertTriangle className="h-4 w-4" />}
        <span>
          {isBlock ? "Concentration limit blocked — override required" : "Concentration warning"}
        </span>
      </div>
      <ul className="space-y-0.5">
        {result.breaches.map((b, i) => {
          const label = GATE_KIND_LABELS[b.kind] ?? b.kind
          const subjectLabel = b.label ? ` · ${b.label}` : ""
          const isCount = b.kind === "factor_concurrent"
          return (
            <li key={`${b.kind}-${b.label}-${i}`} className="font-mono">
              {label}
              {subjectLabel}: {isCount ? b.projected : formatCents(b.projected)} /{" "}
              {isCount ? b.limit : formatCents(b.limit)}
              {b.limit > 0 && (
                <span className="text-foreground/60 ml-1 not-italic">
                  ({Math.round((b.projected / b.limit) * 100)}%)
                </span>
              )}
            </li>
          )
        })}
      </ul>
    </div>
  )
}
