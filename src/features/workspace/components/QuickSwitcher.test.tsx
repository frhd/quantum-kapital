import { describe, it, expect, vi, beforeEach } from "vitest"
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { useEffect } from "react"
import { QuickSwitcher } from "./QuickSwitcher"
import { WorkspaceProvider, useWorkspace } from "../context/WorkspaceContext"
import type { TrackedTicker } from "../../tracker/types"

const watchlistRef: { current: TrackedTicker[] } = { current: [] }
const cachedRef: { current: string[] } = { current: [] }

vi.mock("../../tracker/hooks/useWatchlist", () => ({
  useWatchlist: () => ({
    tickers: watchlistRef.current,
    loading: false,
    error: null,
    refresh: vi.fn(),
    add: vi.fn(),
    remove: vi.fn(),
    setTags: vi.fn(),
    setStatus: vi.fn(),
  }),
}))

vi.mock("../../../shared/api/ibkr", () => ({
  ibkrApi: {
    getCachedTickers: () => Promise.resolve(cachedRef.current),
  },
}))

function makeTicker(symbol: string): TrackedTicker {
  return {
    symbol,
    source: "manual",
    source_meta: null,
    tags: [],
    status: "watchlist",
    in_play_until: null,
    notes: null,
    added_at: "2026-05-03T00:00:00Z",
    updated_at: "2026-05-03T00:00:00Z",
  } as unknown as TrackedTicker
}

function PrimeRecents({ symbols }: { symbols: string[] }) {
  const { navigate } = useWorkspace()
  useEffect(() => {
    for (const s of symbols) navigate(s)
  }, [navigate, symbols])
  return null
}

function CaptureNavigate({ onNavigate }: { onNavigate: (symbol: string | null) => void }) {
  const { symbol } = useWorkspace()
  useEffect(() => {
    onNavigate(symbol)
  }, [symbol, onNavigate])
  return null
}

function pressMetaK() {
  fireEvent.keyDown(window, { key: "k", metaKey: true })
}

describe("QuickSwitcher", () => {
  beforeEach(() => {
    localStorage.clear()
    watchlistRef.current = []
    cachedRef.current = []
  })

  it("is not rendered until the keyboard shortcut fires", () => {
    render(
      <WorkspaceProvider>
        <QuickSwitcher />
      </WorkspaceProvider>,
    )
    expect(screen.queryByTestId("quick-switcher-input")).toBeNull()
  })

  it("opens on Cmd+K and closes on Escape", async () => {
    render(
      <WorkspaceProvider>
        <QuickSwitcher />
      </WorkspaceProvider>,
    )

    act(() => {
      pressMetaK()
    })
    expect(await screen.findByTestId("quick-switcher-input")).toBeInTheDocument()

    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" })
    await waitFor(() => {
      expect(screen.queryByTestId("quick-switcher-input")).toBeNull()
    })
  })

  it("opens on Ctrl+K (non-mac shortcut)", async () => {
    render(
      <WorkspaceProvider>
        <QuickSwitcher />
      </WorkspaceProvider>,
    )

    act(() => {
      fireEvent.keyDown(window, { key: "k", ctrlKey: true })
    })
    expect(await screen.findByTestId("quick-switcher-input")).toBeInTheDocument()
  })

  it("dedupes the universe across recents, watchlist, and cached, recents-first", async () => {
    watchlistRef.current = [makeTicker("MSFT"), makeTicker("AAPL")]
    cachedRef.current = ["GOOG", "AAPL", "TSLA"]

    render(
      <WorkspaceProvider>
        <PrimeRecents symbols={["AAPL", "NVDA"]} />
        <QuickSwitcher />
      </WorkspaceProvider>,
    )

    act(() => {
      pressMetaK()
    })

    const list = await screen.findByTestId("quick-switcher-list")
    await waitFor(() => {
      expect(list.querySelectorAll('[role="option"]')).toHaveLength(5)
    })
    const symbols = Array.from(list.querySelectorAll('[role="option"]')).map(
      (el) => el.textContent?.replace(/(?:RECENT|WATCHLIST|CACHED)/i, "").trim() ?? "",
    )
    expect(symbols).toEqual(["NVDA", "AAPL", "MSFT", "GOOG", "TSLA"])
  })

  it("filters results by case-insensitive substring", async () => {
    cachedRef.current = ["AAPL", "MSFT", "GOOGL", "GOOG"]

    render(
      <WorkspaceProvider>
        <QuickSwitcher />
      </WorkspaceProvider>,
    )
    act(() => pressMetaK())

    const input = await screen.findByTestId("quick-switcher-input")
    fireEvent.change(input, { target: { value: "goo" } })

    await waitFor(() => {
      expect(screen.getByTestId("quick-switcher-row-GOOG")).toBeInTheDocument()
      expect(screen.getByTestId("quick-switcher-row-GOOGL")).toBeInTheDocument()
      expect(screen.queryByTestId("quick-switcher-row-AAPL")).toBeNull()
    })
  })

  it("navigates via useTickerNavigate on Enter and closes the panel", async () => {
    cachedRef.current = ["AAPL", "MSFT"]
    let observed: string | null = null

    render(
      <WorkspaceProvider>
        <CaptureNavigate onNavigate={(s) => (observed = s)} />
        <QuickSwitcher />
      </WorkspaceProvider>,
    )
    act(() => pressMetaK())

    await screen.findByTestId("quick-switcher-row-AAPL")
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Enter" })

    await waitFor(() => {
      expect(observed).toBe("AAPL")
      expect(screen.queryByTestId("quick-switcher-input")).toBeNull()
    })
  })

  it("ArrowDown moves the highlight before Enter selects", async () => {
    cachedRef.current = ["AAPL", "MSFT", "TSLA"]
    let observed: string | null = null

    render(
      <WorkspaceProvider>
        <CaptureNavigate onNavigate={(s) => (observed = s)} />
        <QuickSwitcher />
      </WorkspaceProvider>,
    )
    act(() => pressMetaK())

    await screen.findByTestId("quick-switcher-row-MSFT")
    const dialog = screen.getByRole("dialog")
    fireEvent.keyDown(dialog, { key: "ArrowDown" })
    fireEvent.keyDown(dialog, { key: "Enter" })

    await waitFor(() => {
      expect(observed).toBe("MSFT")
    })
  })

  it("clicking a row commits the symbol and closes the panel", async () => {
    cachedRef.current = ["AAPL", "MSFT"]
    let observed: string | null = null

    render(
      <WorkspaceProvider>
        <CaptureNavigate onNavigate={(s) => (observed = s)} />
        <QuickSwitcher />
      </WorkspaceProvider>,
    )
    act(() => pressMetaK())

    const row = await screen.findByTestId("quick-switcher-row-MSFT")
    fireEvent.mouseDown(row)

    await waitFor(() => {
      expect(observed).toBe("MSFT")
      expect(screen.queryByTestId("quick-switcher-input")).toBeNull()
    })
  })

  it("shows an empty-state row when nothing matches the query", async () => {
    cachedRef.current = ["AAPL", "MSFT"]

    render(
      <WorkspaceProvider>
        <QuickSwitcher />
      </WorkspaceProvider>,
    )
    act(() => pressMetaK())

    const input = await screen.findByTestId("quick-switcher-input")
    fireEvent.change(input, { target: { value: "zzz" } })

    expect(await screen.findByText(/No matches for "zzz"/)).toBeInTheDocument()
  })
})
