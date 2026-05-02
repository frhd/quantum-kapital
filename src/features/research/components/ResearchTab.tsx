import { useState } from "react"
import { FileText, Loader2 } from "lucide-react"

import { Card, CardContent, CardHeader, CardTitle } from "../../../shared/components/ui/card"
import { Input } from "../../../shared/components/ui/input"
import { useResearchNotes } from "../hooks/useResearchNotes"
import { NoteCard } from "./NoteCard"

/**
 * Phase 02 — research notes list view.
 *
 * Renders LLM-authored research notes (and notes attached to alert
 * decisions) newest-first. Symbol filter is client-side over the
 * server-paged list; refresh hooks into the
 * `research-note-written` event so the UI doesn't lag the agent.
 */
export function ResearchTab() {
  const [symbol, setSymbol] = useState("")
  const trimmed = symbol.trim()
  const { notes, loading, error } = useResearchNotes({
    symbol: trimmed || null,
  })

  return (
    <div className="space-y-4">
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <FileText className="h-5 w-5" />
            Research Notes
          </CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="flex items-center gap-2">
            <Input
              placeholder="Filter by symbol (optional)"
              value={symbol}
              onChange={(e) => setSymbol(e.target.value.toUpperCase())}
              className="max-w-xs"
            />
            {loading && <Loader2 className="text-muted-foreground h-4 w-4 animate-spin" />}
          </div>

          {error && <p className="text-destructive text-sm">{error}</p>}

          {!loading && notes.length === 0 && !error && (
            <p className="text-muted-foreground text-sm">
              No research notes yet. Notes appear here when the headless agent or an interactive
              Claude Code session writes one through the MCP `write_research_note` tool.
            </p>
          )}

          <ul className="space-y-3">
            {notes.map((note) => (
              <NoteCard key={note.id} note={note} />
            ))}
          </ul>
        </CardContent>
      </Card>
    </div>
  )
}
