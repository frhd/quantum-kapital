import type { EvidenceRef, ResearchNote } from "../types"
import type { WorkspaceTabId } from "../../workspace/types"
import { useTickerNavigate } from "../../workspace/hooks/useTickerNavigate"
import { MarkdownBody } from "./MarkdownBody"
import { NoteValidityCard } from "./NoteValidityCard"

interface NoteCardProps {
  note: ResearchNote
}

export function NoteCard({ note }: NoteCardProps) {
  const navigate = useTickerNavigate()
  return (
    <li className="border-border bg-card rounded-md border p-3">
      <div className="flex items-baseline justify-between gap-2">
        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={() => navigate(note.symbol, "research")}
            className="font-mono text-sm font-semibold underline-offset-2 hover:underline"
            title="Open in workspace"
          >
            {note.symbol}
          </button>
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

      <NoteValidityCard note={note} />

      <MarkdownBody markdown={note.body_md} />

      {note.evidence_refs.length > 0 && (
        <div className="text-muted-foreground mt-2 flex flex-wrap gap-1 text-xs">
          {note.evidence_refs.map((ref, idx) => {
            const dest = evidenceDestination(ref, note.symbol)
            const label = evidenceLabel(ref)
            if (!dest) {
              return (
                <span key={idx} className="border-border rounded border px-1.5 py-0.5">
                  {label}
                </span>
              )
            }
            return (
              <button
                key={idx}
                type="button"
                onClick={() => navigate(dest.symbol, dest.tab)}
                className="border-border rounded border px-1.5 py-0.5 underline-offset-2 hover:underline"
                title={`Open ${dest.symbol} in workspace`}
              >
                {label}
              </button>
            )
          })}
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

function evidenceDestination(
  ref: EvidenceRef,
  noteSymbol: string,
): { symbol: string; tab: WorkspaceTabId } | null {
  switch (ref.type) {
    case "alert":
      return { symbol: noteSymbol, tab: "alerts" }
    case "news":
      return { symbol: noteSymbol, tab: "news" }
    case "setup":
      return { symbol: noteSymbol, tab: "watchlist" }
    case "bar_range":
      return null
  }
}
