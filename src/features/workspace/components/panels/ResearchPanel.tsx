import { Loader2 } from "lucide-react"
import { Card, CardContent, CardHeader, CardTitle } from "../../../../shared/components/ui/card"
import { useResearchNotes } from "../../../research/hooks/useResearchNotes"
import { NoteCard } from "../../../research/components/NoteCard"
import { useWorkspace } from "../../context/WorkspaceContext"
import { EmptyState } from "../EmptyState"

/**
 * Workspace Phase 2 — research notes panel scoped to the active
 * symbol. Reuses `useResearchNotes({ symbol })` (already filterable)
 * and the extracted `NoteCard` so this view stays in lockstep with
 * the global Research tab.
 */
export function ResearchPanel() {
  const { symbol } = useWorkspace()
  const { notes, loading, error } = useResearchNotes({ symbol: symbol ?? null })

  if (!symbol) {
    return (
      <EmptyState
        title="No symbol selected"
        description="Search for a ticker above to load its research notes."
      />
    )
  }

  if (error) {
    return (
      <EmptyState
        title="Failed to load research notes"
        description={error}
      />
    )
  }

  if (!loading && notes.length === 0) {
    return (
      <EmptyState
        title={`No research notes for ${symbol} yet`}
        description="Notes appear here when the headless agent or an interactive Claude Code session writes one through the MCP write_research_note tool."
      />
    )
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          Research notes
          {loading && <Loader2 className="text-muted-foreground h-4 w-4 animate-spin" />}
        </CardTitle>
      </CardHeader>
      <CardContent>
        <ul className="space-y-3">
          {notes.map((note) => (
            <NoteCard key={note.id} note={note} />
          ))}
        </ul>
      </CardContent>
    </Card>
  )
}
