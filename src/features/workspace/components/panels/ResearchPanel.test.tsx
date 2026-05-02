import { describe, it, expect, vi, beforeEach } from "vitest"
import { render, screen, waitFor } from "@testing-library/react"
import { useEffect } from "react"
import { ResearchPanel } from "./ResearchPanel"
import { WorkspaceProvider, useWorkspace } from "../../context/WorkspaceContext"
import type { ResearchNote } from "../../../research/types"

const useResearchNotesMock = vi.fn()

vi.mock("../../../research/hooks/useResearchNotes", () => ({
  useResearchNotes: (args: { symbol: string | null }) => useResearchNotesMock(args),
}))

vi.mock("../../../research/components/NoteCard", () => ({
  NoteCard: ({ note }: { note: ResearchNote }) => (
    <li data-testid="note-card">
      {note.symbol}#{note.id}
    </li>
  ),
}))

function SetSymbol({ symbol }: { symbol: string | null }) {
  const { setSymbol } = useWorkspace()
  useEffect(() => {
    setSymbol(symbol)
  }, [setSymbol, symbol])
  return null
}

function renderPanel(symbol: string | null) {
  return render(
    <WorkspaceProvider>
      <SetSymbol symbol={symbol} />
      <ResearchPanel />
    </WorkspaceProvider>,
  )
}

const sampleNote: ResearchNote = {
  id: 7,
  symbol: "AAPL",
  body_md: "Test thesis body",
  conviction: "B",
  evidence_refs: [],
  written_by: "agent",
  written_at: "2026-05-02T00:00:00Z",
  setup_id: null,
  alert_id: null,
}

describe("ResearchPanel", () => {
  beforeEach(() => {
    useResearchNotesMock.mockReset()
    useResearchNotesMock.mockReturnValue({ notes: [], loading: false, error: null })
  })

  it("accepts no props — reads from workspace context only", () => {
    // The panel signature MUST be parameter-less so callers can never
    // pass a competing symbol. Workspace invariants forbid panels from
    // taking a `symbol` prop.
    expect(ResearchPanel.length).toBe(0)
  })

  it("shows the no-symbol empty state when workspace has no active symbol", () => {
    renderPanel(null)
    expect(screen.getByText(/No symbol selected/)).toBeInTheDocument()
  })

  it("renders symbol-scoped notes from useResearchNotes", async () => {
    useResearchNotesMock.mockReturnValue({
      notes: [sampleNote],
      loading: false,
      error: null,
    })
    renderPanel("AAPL")
    await waitFor(() => {
      expect(screen.getByTestId("note-card")).toHaveTextContent("AAPL#7")
    })
    expect(useResearchNotesMock).toHaveBeenCalledWith({ symbol: "AAPL" })
  })

  it("renders the empty state when the symbol has no notes", async () => {
    renderPanel("AAPL")
    await waitFor(() => {
      expect(screen.getByText(/No research notes for AAPL yet/)).toBeInTheDocument()
    })
  })

  it("renders the error state when useResearchNotes returns an error", async () => {
    useResearchNotesMock.mockReturnValue({
      notes: [],
      loading: false,
      error: "boom",
    })
    renderPanel("AAPL")
    await waitFor(() => {
      expect(screen.getByText(/Failed to load research notes/)).toBeInTheDocument()
    })
    expect(screen.getByText("boom")).toBeInTheDocument()
  })
})
