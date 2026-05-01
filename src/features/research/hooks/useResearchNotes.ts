import { useCallback, useEffect, useState } from "react"
import { listen, type UnlistenFn } from "@tauri-apps/api/event"

import { ibkrApi } from "../../../shared/api/ibkr"
import type { ResearchNote, ResearchNoteWrittenPayload } from "../types"

const PAGE_SIZE = 50

interface UseResearchNotesArgs {
  symbol?: string | null
}

interface UseResearchNotesResult {
  notes: ResearchNote[]
  loading: boolean
  error: string | null
  refresh: () => Promise<void>
}

/**
 * Phase 02 — minimal SWR-style hook for the research notes feed.
 *
 * Refreshes on mount, on `symbol` change, and whenever a new
 * `research-note-written` event lands so the UI doesn't lag the agent.
 */
export function useResearchNotes({ symbol }: UseResearchNotesArgs = {}): UseResearchNotesResult {
  const [notes, setNotes] = useState<ResearchNote[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const refresh = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      const rows = await ibkrApi.research.listNotes({
        symbol: symbol ?? null,
        limit: PAGE_SIZE,
      })
      setNotes(rows)
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setLoading(false)
    }
  }, [symbol])

  useEffect(() => {
    void refresh()
  }, [refresh])

  useEffect(() => {
    let unlisten: UnlistenFn | undefined
    void (async () => {
      unlisten = await listen<ResearchNoteWrittenPayload>("research-note-written", () => {
        void refresh()
      })
    })()
    return () => {
      unlisten?.()
    }
  }, [refresh])

  return { notes, loading, error, refresh }
}
