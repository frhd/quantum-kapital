import { TAG_WEIGHTS, type BehavioralTag } from "../types"

function classFor(tag: BehavioralTag): string {
  const w = TAG_WEIGHTS[tag]
  if (w > 0) return "bg-emerald-500/15 text-emerald-300 border-emerald-500/30"
  if (w < 0) return "bg-red-500/15 text-red-300 border-red-500/30"
  return "bg-secondary text-foreground/80 border-border"
}

export function BehavioralTagChip({ tag }: { tag: BehavioralTag }) {
  const w = TAG_WEIGHTS[tag]
  const sign = w > 0 ? `+${w}` : `${w}`
  return (
    <span
      className={`inline-flex items-center gap-1 rounded-sm border px-1.5 py-0.5 font-mono text-[10px] tracking-tight ${classFor(tag)}`}
      title={`weight ${sign}`}
    >
      <span>{tag}</span>
      <span className="opacity-70">{sign}</span>
    </span>
  )
}
