import type { Alert } from "../types"
import { ALERT_KIND_LABELS } from "../types"

interface AlertRowProps {
  alert: Alert
  onClick: () => void
}

function relativeTime(iso: string): string {
  const t = new Date(iso).getTime()
  if (Number.isNaN(t)) return ""
  const diffMs = Date.now() - t
  const sec = Math.round(diffMs / 1000)
  if (sec < 60) return `${sec}s ago`
  const min = Math.round(sec / 60)
  if (min < 60) return `${min}m ago`
  const hr = Math.round(min / 60)
  if (hr < 24) return `${hr}h ago`
  const day = Math.round(hr / 24)
  return `${day}d ago`
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

  return (
    <button
      type="button"
      onClick={onClick}
      className={
        "group flex w-full items-start gap-3 rounded-md border px-3 py-2 text-left transition-colors " +
        (alert.seen
          ? "border-slate-700/60 bg-slate-900/40 hover:bg-slate-800/60"
          : "border-slate-700 bg-slate-800/70 hover:bg-slate-800")
      }
    >
      <div className="flex flex-1 flex-col gap-1 overflow-hidden">
        <div className="flex items-center gap-2">
          {!alert.seen && (
            <span aria-hidden className="size-2 shrink-0 rounded-full bg-blue-400" title="Unseen" />
          )}
          <span className="font-mono text-sm font-semibold text-white">{symbol}</span>
          <span
            className={
              "rounded-full border px-2 py-0.5 text-[10px] tracking-wide uppercase " +
              kindAccent(alert.kind)
            }
          >
            {ALERT_KIND_LABELS[alert.kind]}
          </span>
          <span className="ml-auto text-xs text-slate-500">{relativeTime(alert.fired_at)}</span>
        </div>
        <p className="truncate text-xs text-slate-300 group-hover:text-slate-100" title={summary}>
          {summary}
        </p>
      </div>
    </button>
  )
}
