import { useState } from "react"
import { FileText, Loader2 } from "lucide-react"

import { Card, CardContent, CardHeader, CardTitle } from "../../../shared/components/ui/card"
import { Input } from "../../../shared/components/ui/input"
import { useResearchNotes } from "../hooks/useResearchNotes"
import type { EvidenceRef, ResearchNote } from "../types"

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

function NoteCard({ note }: { note: ResearchNote }) {
  return (
    <li className="border-border bg-card rounded-md border p-3">
      <div className="flex items-baseline justify-between gap-2">
        <div className="flex items-center gap-2">
          <span className="font-mono text-sm font-semibold">{note.symbol}</span>
          {note.conviction && (
            <span className="border-border text-muted-foreground rounded border px-1.5 py-0.5 text-xs">
              Conviction {note.conviction}
            </span>
          )}
        </div>
        <span className="text-muted-foreground text-xs">
          {new Date(note.written_at).toLocaleString()} · {note.written_by}
        </span>
      </div>

      <pre className="text-foreground/90 mt-2 font-sans text-sm whitespace-pre-wrap">
        {note.body_md}
      </pre>

      {note.evidence_refs.length > 0 && (
        <div className="text-muted-foreground mt-2 flex flex-wrap gap-1 text-xs">
          {note.evidence_refs.map((ref, idx) => (
            <span key={idx} className="border-border rounded border px-1.5 py-0.5">
              {evidenceLabel(ref)}
            </span>
          ))}
        </div>
      )}
    </li>
  )
}

function evidenceLabel(ref: EvidenceRef): string {
  switch (ref.type) {
    case "alert":
      return `alert#${ref.id}`
    case "news":
      return `news#${ref.cache_id}`
    case "setup":
      return `setup#${ref.id}`
    case "bar_range":
      return `${ref.symbol} ${ref.from}→${ref.to}`
  }
}
