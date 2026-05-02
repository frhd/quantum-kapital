import { describe, it, expect, vi, beforeEach } from "vitest"
import { render, screen, waitFor } from "@testing-library/react"
import { useEffect } from "react"
import { HistoryPanel } from "./HistoryPanel"
import { WorkspaceProvider, useWorkspace } from "../../context/WorkspaceContext"
import type { PredictionWithOutcome } from "../../../eval/types"
import type { TickerHistorySummary } from "../../hooks/useTickerHistory"

const useTickerHistoryMock = vi.fn()

vi.mock("../../hooks/useTickerHistory", () => ({
  useTickerHistory: (symbol: string | null) => useTickerHistoryMock(symbol),
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
      <HistoryPanel />
    </WorkspaceProvider>,
  )
}

const emptySummary: TickerHistorySummary = {
  total: 0,
  pending: 0,
  skipped: 0,
  unparseable: 0,
  scoreable: 0,
  hits: 0,
  hitRate: null,
}

const baseHookReturn = {
  rows: [] as PredictionWithOutcome[],
  summary: emptySummary,
  loading: false,
  error: null as string | null,
  windowDays: 90,
  setWindowDays: vi.fn(),
  refresh: vi.fn(),
}

const sampleRow: PredictionWithOutcome = {
  prediction: {
    id: 1,
    source: "morning_pack",
    symbol: "AAPL",
    conviction: "B",
    entry_zone: "190-192",
    invalidation: "188",
    target: "200",
    thesis_md: "Strong earnings",
    morning_pack_id: "2026-05-01",
    predicted_at: "2026-05-01T13:00:00Z",
  },
  outcome: {
    id: 11,
    pack_date: "2026-05-01",
    symbol: "AAPL",
    outcome_class: "hit_target",
    conviction: "B",
    entry_zone_low: 190,
    entry_zone_high: 192,
    invalidation_lvl: 188,
    realized_high: 205,
    realized_low: 189,
    realized_close: 201,
    eval_window_days: 5,
    evaluated_at: "2026-05-06T20:00:00Z",
    prediction_id: 1,
  },
}

const pendingRow: PredictionWithOutcome = {
  prediction: {
    ...sampleRow.prediction,
    id: 2,
    predicted_at: "2026-05-02T13:00:00Z",
  },
  outcome: null,
}

describe("HistoryPanel", () => {
  beforeEach(() => {
    useTickerHistoryMock.mockReset()
    useTickerHistoryMock.mockReturnValue({ ...baseHookReturn })
  })

  it("accepts no props — reads from workspace context only", () => {
    expect(HistoryPanel.length).toBe(0)
  })

  it("shows the no-symbol empty state when workspace has no active symbol", () => {
    renderPanel(null)
    expect(screen.getByText(/No symbol selected/)).toBeInTheDocument()
  })

  it("passes the active symbol to useTickerHistory", async () => {
    renderPanel("AMD")
    await waitFor(() => {
      expect(useTickerHistoryMock).toHaveBeenCalled()
    })
    const last = useTickerHistoryMock.mock.calls[useTickerHistoryMock.mock.calls.length - 1]
    expect(last?.[0]).toBe("AMD")
  })

  it("renders the empty state when no predictions are returned", async () => {
    renderPanel("AAPL")
    await waitFor(() => {
      expect(screen.getByText(/No predictions for AAPL in the last 90 days/)).toBeInTheDocument()
    })
  })

  it("renders the prediction rows + accuracy headline", async () => {
    useTickerHistoryMock.mockReturnValue({
      ...baseHookReturn,
      rows: [sampleRow, pendingRow],
      summary: {
        total: 2,
        pending: 1,
        skipped: 0,
        unparseable: 0,
        scoreable: 1,
        hits: 1,
        hitRate: 1,
      },
    })
    renderPanel("AAPL")
    await waitFor(() => {
      expect(screen.getByText("100.0%")).toBeInTheDocument()
    })
    expect(screen.getByText("hit target")).toBeInTheDocument()
    expect(screen.getAllByText("pending").length).toBeGreaterThan(0)
    expect(screen.getByText(/1 \/ 1 scoreable/)).toBeInTheDocument()
  })

  it("renders the no-resolved hint when scoreable is 0", async () => {
    useTickerHistoryMock.mockReturnValue({
      ...baseHookReturn,
      rows: [pendingRow],
      summary: {
        total: 1,
        pending: 1,
        skipped: 0,
        unparseable: 0,
        scoreable: 0,
        hits: 0,
        hitRate: null,
      },
    })
    renderPanel("AAPL")
    await waitFor(() => {
      expect(screen.getByText("—")).toBeInTheDocument()
    })
    expect(screen.getByText(/no resolved predictions yet/)).toBeInTheDocument()
  })

  it("renders the error state when useTickerHistory surfaces an error", async () => {
    useTickerHistoryMock.mockReturnValue({ ...baseHookReturn, error: "boom" })
    renderPanel("AAPL")
    await waitFor(() => {
      expect(screen.getByText(/Failed to load history/)).toBeInTheDocument()
    })
    expect(screen.getByText("boom")).toBeInTheDocument()
  })
})
