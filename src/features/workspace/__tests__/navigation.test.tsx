/**
 * Phase 4 cross-feature contract — every list view that exposes a
 * symbol must route into the workspace via `useTickerNavigate(symbol,
 * tab?)`. We render each entry point with fixture data, click the
 * symbol target, and assert the workspace context received the expected
 * `(symbol, tab)`. If a future refactor adds a parallel navigation
 * pathway (e.g. a bespoke prop), one of these tests will fail loudly.
 */
import { describe, it, expect, vi, beforeEach } from "vitest"
import { render, screen, fireEvent, act } from "@testing-library/react"
import { useEffect } from "react"
import { WorkspaceProvider, useWorkspace } from "../context/WorkspaceContext"
import type { WorkspaceTabId } from "../types"
import { Watchlist } from "../../tracker/components/Watchlist"
import { MorningPackPanel } from "../../tracker/components/MorningPack"
import { AlertRow } from "../../tracker/components/AlertRow"
import { AlertFeed } from "../../tracker/components/AlertFeed"
import { ScannerResults } from "../../scanner/components/ScannerResults"
import { NoteCard } from "../../research/components/NoteCard"
import { CandidateBrowser } from "../../candidates/components/CandidateBrowser"
import type { Alert } from "../../tracker/types"
import type { TrackedTicker, MorningPack as MorningPackType } from "../../tracker/types"
import type { ScannerData } from "../../../shared/types"
import type { ResearchNote } from "../../research/types"
import type { Candidate } from "../../candidates/types"

// ---------------------------------------------------------------- mocks

vi.mock("../../tracker/hooks/useAlerts", () => ({
  useAlerts: () => ({
    alerts: alertsRef.current,
    loading: false,
    error: null,
    unseenCount: 0,
    hasMore: false,
    refresh: vi.fn(),
    loadMore: vi.fn(),
    markAllSeen: vi.fn(),
    markOneSeen: vi.fn(),
  }),
}))

const morningPackRef: { current: MorningPackType | null } = { current: null }
vi.mock("../../../shared/api/ibkr", () => ({
  ibkrApi: {
    tracker: {
      getMorningPack: () => Promise.resolve(morningPackRef.current),
    },
    candidates: {
      list: () => Promise.resolve(candidatesRef.current),
      promote: vi.fn(),
      refresh: vi.fn(),
    },
  },
}))

const candidatesRef: { current: Candidate[] } = { current: [] }
const alertsRef: { current: Alert[] } = { current: [] }

vi.mock("../../candidates/hooks/useCandidates", () => ({
  useCandidates: () => ({
    candidates: candidatesRef.current,
    loading: false,
    error: null,
    refresh: vi.fn(),
    triggerRefresh: vi.fn(),
    refreshing: false,
  }),
}))

// ---------------------------------------------------------------- helpers

interface ProbeState {
  symbol: string | null
  tab: WorkspaceTabId
  pageNavigations: number
}

function makeProbe() {
  const state: ProbeState = { symbol: null, tab: "overview", pageNavigations: 0 }
  function Probe() {
    const ws = useWorkspace()
    useEffect(() => {
      state.symbol = ws.symbol
      state.tab = ws.tab
    }, [ws.symbol, ws.tab])
    return null
  }
  return { state, Probe }
}

function renderInWorkspace(node: React.ReactNode, onNavigatePage?: () => void) {
  const { state, Probe } = makeProbe()
  const utils = render(
    <WorkspaceProvider onNavigatePage={onNavigatePage}>
      <Probe />
      {node}
    </WorkspaceProvider>,
  )
  return { ...utils, state }
}

// ---------------------------------------------------------------- fixtures

const tracked: TrackedTicker = {
  symbol: "AAPL",
  source: "manual",
  source_meta: null,
  status: "watching",
  tags: [],
  notes: null,
  added_at: "2026-05-01T12:00:00Z",
  last_checked_at: null,
  in_play_until: null,
  cool_down_until: null,
  archived_at: null,
}

const scannerRow: ScannerData = {
  rank: 1,
  leg: "",
  contract: {
    contract_id: 1,
    symbol: "MSFT",
    sec_type: "Stock",
    exchange: "NASDAQ",
    primary_exchange: "NASDAQ",
    currency: "USD",
    local_symbol: "MSFT",
    trading_class: "MSFT",
    min_tick: 0.01,
    multiplier: "1",
    price_magnifier: 1,
  },
}

const noteAlertRef: ResearchNote = {
  id: 1,
  symbol: "TSLA",
  body_md: "thesis",
  conviction: "B",
  evidence_refs: [
    { type: "alert", id: 99 },
    { type: "news", cache_id: 7 },
    { type: "setup", id: 12 },
    { type: "bar_range", symbol: "TSLA", from: "2026-05-01", to: "2026-05-02" },
  ],
  written_by: "agent",
  written_at: "2026-05-02T15:00:00Z",
  setup_id: null,
  alert_id: null,
}

const sampleAlert: Alert = {
  id: 42,
  setup_id: 1,
  kind: "detected",
  fired_at: "2026-05-02T00:00:00Z",
  payload: { symbol: "NVDA" },
  seen: false,
}

const morningPack: MorningPackType = {
  date: "2026-05-02",
  generated_at: "2026-05-02T20:00:00Z",
  ranked: [{ setup_id: 1, rank: 1, why_top_pick: "strong volume" }],
}

// ---------------------------------------------------------------- tests

describe("Phase 4 — universal navigation", () => {
  beforeEach(() => {
    candidatesRef.current = []
    morningPackRef.current = null
    alertsRef.current = []
  })

  it("Watchlist row → (symbol, overview)", () => {
    const { state } = renderInWorkspace(
      <Watchlist
        tickers={[tracked]}
        loading={false}
        error={null}
        onRemove={vi.fn()}
        onSaveTags={vi.fn()}
      />,
    )
    fireEvent.click(screen.getByTitle("Open in workspace"))
    expect(state.symbol).toBe("AAPL")
    expect(state.tab).toBe("overview")
  })

  it("MorningPack ticker → (symbol, overview)", async () => {
    morningPackRef.current = morningPack
    const { state, findByTitle } = renderInWorkspace(
      <MorningPackPanel
        lastMorningPackReady={null}
        activeSetupBySymbol={{
          AAPL: {
            id: 1,
            symbol: "AAPL",
            strategy: "breakout",
            direction: "long",
            detected_at: "2026-05-02T20:00:00Z",
            trigger_price: 100,
            stop_price: 95,
            targets: [],
            raw_signals: null,
            thesis: null,
            thesis_json: null,
            status: "active",
            invalidated_at: null,
            invalidation_reason: null,
            archived_at: null,
          },
        }}
      />,
    )
    const btn = await findByTitle("Open in workspace")
    fireEvent.click(btn)
    expect(state.symbol).toBe("AAPL")
    expect(state.tab).toBe("overview")
  })

  it("AlertFeed row → (alert.symbol, alerts)", () => {
    alertsRef.current = [sampleAlert]
    const { state } = renderInWorkspace(
      <AlertFeed lastSetupDetected={null} lastInvalidated={null} lastStatusChanged={null} />,
    )
    fireEvent.click(screen.getByText("NVDA"))
    expect(state.symbol).toBe("NVDA")
    expect(state.tab).toBe("alerts")
  })

  it("AlertRow standalone → fires its onClick prop without navigating itself", () => {
    const onClick = vi.fn()
    const { state } = renderInWorkspace(<AlertRow alert={sampleAlert} onClick={onClick} />)
    fireEvent.click(screen.getByText("NVDA"))
    expect(onClick).toHaveBeenCalledTimes(1)
    // AlertRow has no direct navigation; routing happens in AlertFeed.
    expect(state.symbol).toBeNull()
  })

  it("ScannerResults Analyze → (symbol, overview)", () => {
    const { state } = renderInWorkspace(
      <ScannerResults
        results={[scannerRow]}
        lastUpdate={null}
        isRunning={false}
        error={null}
        onAddToTracker={vi.fn()}
      />,
    )
    fireEvent.click(screen.getByTitle("Open in analysis"))
    expect(state.symbol).toBe("MSFT")
    expect(state.tab).toBe("overview")
  })

  it("NoteCard symbol header → (note.symbol, research)", () => {
    const { state } = renderInWorkspace(<NoteCard note={noteAlertRef} />)
    fireEvent.click(screen.getByRole("button", { name: /TSLA/ }))
    expect(state.symbol).toBe("TSLA")
    expect(state.tab).toBe("research")
  })

  it("NoteCard evidence chip (alert) → (note.symbol, alerts)", () => {
    const { state } = renderInWorkspace(<NoteCard note={noteAlertRef} />)
    fireEvent.click(screen.getByRole("button", { name: "alert#99" }))
    expect(state.symbol).toBe("TSLA")
    expect(state.tab).toBe("alerts")
  })

  it("NoteCard evidence chip (news) → (note.symbol, news)", () => {
    const { state } = renderInWorkspace(<NoteCard note={noteAlertRef} />)
    fireEvent.click(screen.getByRole("button", { name: "news#7" }))
    expect(state.symbol).toBe("TSLA")
    expect(state.tab).toBe("news")
  })

  it("NoteCard evidence chip (setup) → (note.symbol, watchlist)", () => {
    const { state } = renderInWorkspace(<NoteCard note={noteAlertRef} />)
    fireEvent.click(screen.getByRole("button", { name: "setup#12" }))
    expect(state.symbol).toBe("TSLA")
    expect(state.tab).toBe("watchlist")
  })

  it("NoteCard evidence chip (bar_range) is non-clickable", () => {
    renderInWorkspace(<NoteCard note={noteAlertRef} />)
    const span = screen.getByText(/TSLA 2026-05-01→2026-05-02/)
    // Non-button span — should not be inside a <button>
    expect(span.closest("button")).toBeNull()
  })

  it("CandidateBrowser row symbol → (candidate.symbol, overview)", async () => {
    candidatesRef.current = [
      {
        symbol: "GOOG",
        score: 0.42,
        sources: [],
        reason_md: null,
        first_seen: 1700000000,
        last_seen: 1700000000,
        decay_at: 1700100000,
        promoted_at: null,
      },
    ]
    const { state, findByRole } = renderInWorkspace(<CandidateBrowser />)
    const btn = await findByRole("button", { name: "GOOG" })
    fireEvent.click(btn)
    expect(state.symbol).toBe("GOOG")
    expect(state.tab).toBe("overview")
  })

  it("navigate triggers onNavigatePage so host page swaps to ticker", () => {
    const onNav = vi.fn()
    renderInWorkspace(
      <Watchlist
        tickers={[tracked]}
        loading={false}
        error={null}
        onRemove={vi.fn()}
        onSaveTags={vi.fn()}
      />,
      onNav,
    )
    act(() => {
      fireEvent.click(screen.getByTitle("Open in workspace"))
    })
    expect(onNav).toHaveBeenCalled()
  })
})
