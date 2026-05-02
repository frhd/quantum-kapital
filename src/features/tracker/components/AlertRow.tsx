import { relativeTime } from "../../../shared/lib/relativeTime"
import type { Alert } from "../types"
import { ALERT_KIND_LABELS } from "../types"

interface AlertRowProps {
  alert: Alert
  onClick: () => void
}

type EnrichmentState = "pending" | "ready" | "skipped"

function enrichmentState(alert: Alert): EnrichmentState {
  if (!alert.enriched_at) return "pending"
  return alert.research_note_id ? "ready" : "skipped"
}

const ENRICHMENT_BADGE: Record<EnrichmentState, { label: string; cls: string; title: string }> = {
  pending: {
    label: "Enriching…",
    cls: "border-slate-400/40 bg-slate-500/10 text-slate-300",
    title: "Per-alert deep dive in progress",
  },
  ready: {
    label: "Deep dive",
    cls: "border-blue-400/60 bg-blue-500/15 text-blue-200",
    title: "Click to open the linked research note",
  },
  skipped: {
    label: "Dive skipped",
    cls: "border-amber-400/40 bg-amber-500/10 text-amber-200",
    title: "Deep dive was skipped (e.g. budget guardrail)",
  },
}

function kindAccent(kind: Alert["kind"]): string {
  switch (kind) {
    case "detected":
      return "border-emerald-400/60 text-emerald-300"
    case "invalidated":
      return "border-rose-400/60 text-rose-300"
    case "target_hit":
      return "border-sky-400/60 text-sky-300"
    case "thesis_changed":
      return "border-amber-400/60 text-amber-300"
  }
}

function pickSummary(alert: Alert): string {
  const p = alert.payload
  switch (alert.kind) {
    case "detected": {
      const strategy = typeof p.strategy === "string" ? p.strategy : "setup"
      const direction = typeof p.direction === "string" ? (p.direction as string).toUpperCase() : ""
      const trigger = typeof p.trigger_price === "number" ? p.trigger_price.toFixed(2) : ""
      return `${strategy}${direction ? ` ${direction}` : ""}${trigger ? ` @ $${trigger}` : ""}`
    }
    case "invalidated":
      return typeof p.reason === "string" ? p.reason : "Setup invalidated"
    case "target_hit":
      return "Target hit"
    case "thesis_changed":
      return "Thesis updated"
  }
}

export function AlertRow({ alert, onClick }: AlertRowProps) {
  const symbol = typeof alert.payload.symbol === "string" ? alert.payload.symbol : "—"
  const summary = pickSummary(alert)
  const dive = ENRICHMENT_BADGE[enrichmentState(alert)]

  return (
    <button
      type="button"
      onClick={onClick}
      className={
        "group flex w-full items-start gap-3 rounded-md border px-3 py-2 text-left transition-colors " +
        (alert.seen
          ? "border-border/60 bg-background/40 hover:bg-card/60"
          : "border-border bg-card/70 hover:bg-card")
      }
    >
      <div className="flex flex-1 flex-col gap-1 overflow-hidden">
        <div className="flex items-center gap-2">
          {!alert.seen && (
            <span aria-hidden className="size-2 shrink-0 rounded-full bg-blue-400" title="Unseen" />
          )}
          <span className="text-foreground font-mono text-sm font-semibold">{symbol}</span>
          <span
            className={
              "rounded-full border px-2 py-0.5 text-[10px] tracking-wide uppercase " +
              kindAccent(alert.kind)
            }
          >
            {ALERT_KIND_LABELS[alert.kind]}
          </span>
          <span
            className={
              "rounded-full border px-2 py-0.5 text-[10px] tracking-wide uppercase " + dive.cls
            }
            title={dive.title}
          >
            {dive.label}
          </span>
          <span className="text-muted-foreground ml-auto text-xs">
            {relativeTime(alert.fired_at)}
          </span>
        </div>
        <p className="text-foreground group-hover:text-foreground truncate text-xs" title={summary}>
          {summary}
        </p>
      </div>
    </button>
  )
}
