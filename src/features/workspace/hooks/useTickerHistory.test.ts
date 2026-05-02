import { describe, it, expect, vi, beforeEach } from "vitest"
import { renderHook, waitFor } from "@testing-library/react"
import { useTickerHistory } from "./useTickerHistory"
import type { PredictionWithOutcome } from "../../eval/types"

const predictionHistoryMock = vi.fn()

vi.mock("../../../shared/api/ibkr", () => ({
  ibkrApi: {
    eval: {
      predictionHistory: (symbol: string, windowDays: number) =>
        predictionHistoryMock(symbol, windowDays),
    },
  },
}))

function row(id: number, outcomeClass: string | null): PredictionWithOutcome {
  const prediction = {
    id,
    source: "morning_pack",
    symbol: "AAPL",
    conviction: "B" as const,
    entry_zone: null,
    invalidation: null,
    target: null,
    thesis_md: null,
    morning_pack_id: null,
    predicted_at: "2026-05-01T13:00:00Z",
  }
  if (!outcomeClass) {
    return { prediction, outcome: null }
  }
  return {
    prediction,
    outcome: {
      id: id + 100,
      pack_date: "2026-05-01",
      symbol: "AAPL",
      outcome_class: outcomeClass,
      conviction: "B",
      entry_zone_low: null,
      entry_zone_high: null,
      invalidation_lvl: null,
      realized_high: 0,
      realized_low: 0,
      realized_close: 0,
      eval_window_days: 5,
      evaluated_at: "2026-05-06T20:00:00Z",
      prediction_id: id,
    },
  }
}

describe("useTickerHistory.deriveSummary", () => {
  beforeEach(() => {
    predictionHistoryMock.mockReset()
  })

  it("counts pending, skipped, unparseable, and scoreable outcomes correctly", async () => {
    predictionHistoryMock.mockResolvedValue([
      row(1, "hit_target"),
      row(2, "hit_entry"),
      row(3, "hit_invalidation"),
      row(4, "drifted"),
      row(5, "skipped"),
      row(6, "unparseable"),
      row(7, null),
    ])
    const { result } = renderHook(() => useTickerHistory("AAPL"))
    await waitFor(() => {
      expect(result.current.loading).toBe(false)
    })
    const s = result.current.summary
    expect(s.total).toBe(7)
    expect(s.pending).toBe(1)
    expect(s.skipped).toBe(1)
    expect(s.unparseable).toBe(1)
    // scoreable = hit_target + hit_entry + hit_invalidation + drifted = 4
    expect(s.scoreable).toBe(4)
    expect(s.hits).toBe(2)
    expect(s.hitRate).toBeCloseTo(0.5, 5)
  })

  it("returns null hitRate when no resolved predictions are scoreable", async () => {
    predictionHistoryMock.mockResolvedValue([row(1, null), row(2, "skipped")])
    const { result } = renderHook(() => useTickerHistory("AAPL"))
    await waitFor(() => {
      expect(result.current.loading).toBe(false)
    })
    expect(result.current.summary.hitRate).toBeNull()
    expect(result.current.summary.scoreable).toBe(0)
  })

  it("does not call the API when symbol is null", async () => {
    const { result } = renderHook(() => useTickerHistory(null))
    await waitFor(() => {
      expect(result.current.loading).toBe(false)
    })
    expect(predictionHistoryMock).not.toHaveBeenCalled()
    expect(result.current.rows).toEqual([])
  })

  it("surfaces errors as a string", async () => {
    predictionHistoryMock.mockRejectedValue(new Error("kaboom"))
    const { result } = renderHook(() => useTickerHistory("AAPL"))
    await waitFor(() => {
      expect(result.current.error).toBe("kaboom")
    })
    expect(result.current.rows).toEqual([])
  })
})
