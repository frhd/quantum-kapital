import type { Grade } from "../types"

const GRADE_CLASSES: Record<Grade, string> = {
  A: "bg-emerald-500/20 text-emerald-300 border-emerald-500/40",
  B: "bg-green-500/15 text-green-300 border-green-500/40",
  C: "bg-amber-500/15 text-amber-300 border-amber-500/40",
  D: "bg-orange-500/20 text-orange-300 border-orange-500/40",
  F: "bg-red-500/20 text-red-300 border-red-500/40",
}

export function GradeBadge({ grade, score }: { grade: Grade; score: number }) {
  return (
    <span
      className={`inline-flex items-center gap-2 rounded-md border px-2 py-0.5 font-mono text-sm font-semibold tabular-nums ${GRADE_CLASSES[grade]}`}
      data-testid="grade-badge"
    >
      <span>{grade}</span>
      <span className="text-xs opacity-80">
        {score >= 0 ? `+${score.toFixed(1)}` : score.toFixed(1)}
      </span>
    </span>
  )
}
