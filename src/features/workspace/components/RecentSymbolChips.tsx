import { useWorkspace } from "../context/WorkspaceContext"
import { useTickerNavigate } from "../hooks/useTickerNavigate"

interface RecentSymbolChipsProps {
  /** Optional className passed through to the chip wrapper. */
  className?: string
}

/**
 * Phase 5 — chip strip backed by `WorkspaceContext.recents`. Renders
 * nothing when the list is empty so the workspace empty state stays
 * clean for first-run users.
 */
export function RecentSymbolChips({ className }: RecentSymbolChipsProps) {
  const { recents } = useWorkspace()
  const navigate = useTickerNavigate()

  if (recents.length === 0) return null

  return (
    <div
      className={["flex flex-wrap gap-2", className].filter(Boolean).join(" ")}
      data-testid="recent-symbol-chips"
    >
      {recents.map((sym) => (
        <button
          key={sym}
          type="button"
          onClick={() => navigate(sym)}
          className="border-border bg-card/60 text-foreground hover:bg-card focus-visible:ring-ring rounded-full border px-3 py-1 font-mono text-xs transition-colors focus-visible:ring-2 focus-visible:outline-none"
        >
          {sym}
        </button>
      ))}
    </div>
  )
}
