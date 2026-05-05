import { TAG_WEIGHTS } from "../../trade-review/types"
import type { TagFrequency } from "../types"

function classFor(weight: number): string {
  if (weight > 0) return "bg-emerald-500/40"
  if (weight < 0) return "bg-red-500/40"
  return "bg-secondary"
}

export function TagFrequencyChart({ frequencies }: { frequencies: TagFrequency[] }) {
  if (frequencies.length === 0) {
    return (
      <p className="text-muted-foreground py-4 text-center text-xs">
        No behavioral tags fired in this window.
      </p>
    )
  }
  const max = Math.max(...frequencies.map((f) => f.count))
  return (
    <ul className="space-y-1.5" data-testid="tag-frequency-chart">
      {frequencies.map((f) => {
        const weight = TAG_WEIGHTS[f.tag] ?? 0
        const pct = max > 0 ? (f.count / max) * 100 : 0
        return (
          <li key={f.tag} className="grid grid-cols-[12rem_1fr_3.5rem] items-center gap-2 text-xs">
            <span className="text-foreground/90 truncate font-mono">{f.tag}</span>
            <div className="bg-muted/30 relative h-3 overflow-hidden rounded-sm">
              <div
                className={`absolute inset-y-0 left-0 ${classFor(weight)}`}
                style={{ width: `${pct}%` }}
                aria-hidden
              />
            </div>
            <span className="text-foreground/80 text-right font-mono tabular-nums">
              {f.count} · {(f.pct_of_reviews * 100).toFixed(0)}%
            </span>
          </li>
        )
      })}
    </ul>
  )
}
