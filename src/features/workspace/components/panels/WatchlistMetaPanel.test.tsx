import { describe, it, expect, vi, beforeEach } from "vitest"
import { render, screen, waitFor, fireEvent } from "@testing-library/react"
import { useEffect } from "react"
import { WatchlistMetaPanel } from "./WatchlistMetaPanel"
import { WorkspaceProvider, useWorkspace } from "../../context/WorkspaceContext"
import { AddToTrackerProvider } from "../../context/AddToTrackerContext"
import type { AddToTrackerPrefill, TrackedTicker } from "../../../tracker/types"

const useWatchlistMock = vi.fn()

vi.mock("../../../tracker/hooks/useWatchlist", () => ({
  useWatchlist: () => useWatchlistMock(),
}))

vi.mock("../../../tracker/hooks/useTrackerEvents", () => ({
  useTrackerEvents: () => ({
    recentEvents: [],
    lastSetupDetected: null,
    lastInvalidated: null,
    lastStatusChanged: null,
    lastMorningPackReady: null,
    activeSetupBySymbol: {},
  }),
}))

function SetSymbol({ symbol }: { symbol: string | null }) {
  const { setSymbol } = useWorkspace()
  useEffect(() => {
    setSymbol(symbol)
  }, [setSymbol, symbol])
  return null
}

type AddOpener = (prefill: AddToTrackerPrefill) => void

function renderPanel(symbol: string | null, options: { addOpen?: AddOpener | null } = {}) {
  const open = options.addOpen ?? null
  const tree = (
    <WorkspaceProvider>
      <SetSymbol symbol={symbol} />
      <WatchlistMetaPanel />
    </WorkspaceProvider>
  )
  if (open) {
    return render(<AddToTrackerProvider open={open}>{tree}</AddToTrackerProvider>)
  }
  return render(tree)
}

function makeTicker(overrides: Partial<TrackedTicker> = {}): TrackedTicker {
  return {
    symbol: "AAPL",
    source: "manual",
    source_meta: null,
    status: "watching",
    tags: ["breakout"],
    notes: null,
    added_at: "2026-04-30T12:00:00Z",
    last_checked_at: null,
    in_play_until: null,
    cool_down_until: null,
    archived_at: null,
    ...overrides,
  }
}

describe("WatchlistMetaPanel", () => {
  beforeEach(() => {
    useWatchlistMock.mockReset()
    useWatchlistMock.mockReturnValue({
      tickers: [],
      loading: false,
      error: null,
      setTags: vi.fn(),
    })
  })

  it("accepts no props — reads from workspace context only", () => {
    expect(WatchlistMetaPanel.length).toBe(0)
  })

  it("shows the no-symbol empty state when workspace has no active symbol", () => {
    renderPanel(null)
    expect(screen.getByText(/No symbol selected/)).toBeInTheDocument()
  })

  it("renders an Add-to-tracker CTA when the active symbol is not on the watchlist", async () => {
    const open: AddOpener = vi.fn()
    renderPanel("NVDA", { addOpen: open })
    await waitFor(() => {
      expect(screen.getByText(/NVDA is not on your tracker/)).toBeInTheDocument()
    })
    fireEvent.click(screen.getByRole("button", { name: /Add to tracker/ }))
    expect(open).toHaveBeenCalledWith({ symbol: "NVDA", source: "manual" })
  })

  it("renders ticker metadata when the symbol is tracked", async () => {
    useWatchlistMock.mockReturnValue({
      tickers: [makeTicker()],
      loading: false,
      error: null,
      setTags: vi.fn(),
    })
    renderPanel("AAPL")
    await waitFor(() => {
      expect(screen.getByText(/Watchlist/)).toBeInTheDocument()
    })
    expect(screen.getByText("Watching")).toBeInTheDocument()
    expect(screen.getByText("manual")).toBeInTheDocument()
    expect(screen.getByText("Breakout")).toBeInTheDocument()
  })

  it("renders an error empty state when useWatchlist surfaces an error", async () => {
    useWatchlistMock.mockReturnValue({
      tickers: [],
      loading: false,
      error: "boom",
      setTags: vi.fn(),
    })
    renderPanel("AAPL")
    await waitFor(() => {
      expect(screen.getByText(/Failed to load watchlist/)).toBeInTheDocument()
    })
    expect(screen.getByText("boom")).toBeInTheDocument()
  })
})
