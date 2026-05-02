import { describe, it, expect, vi, beforeEach } from "vitest"
import { renderHook, waitFor } from "@testing-library/react"
import { StrictMode, createElement, type ReactNode } from "react"
import { useProjections } from "./useProjections"

const generateProjectionResultsMock = vi.fn()
const getFundamentalDataMock = vi.fn()

vi.mock("../../../shared/api/ibkr", () => ({
  ibkrApi: {
    generateProjectionResults: (symbol: string, assumptions?: unknown) =>
      generateProjectionResultsMock(symbol, assumptions),
    // Phase 1: this MUST NOT be called from useProjections any more.
    // The mock is wired so a stray call would surface in the assertions.
    getFundamentalData: (symbol: string) => getFundamentalDataMock(symbol),
  },
}))

const sampleBundle = {
  fundamentals: {
    symbol: "AAPL",
    historical: [],
    currentMetrics: { peRatio: 30, sharesOutstanding: 15000 },
  },
  results: {
    baseline: {
      year: 2024,
      revenue: 0,
      revenueGrowth: 0,
      netIncome: 0,
      netIncomeGrowth: null,
      netIncomeMargins: 0,
      eps: 0,
      peLowEst: 0,
      peHighEst: 0,
      sharePriceLow: 0,
      sharePriceHigh: 0,
      valuationMethod: "P/E",
    },
    projections: [],
    cagr: {
      bear: { revenue: 0, sharePrice: 0 },
      base: { revenue: 0, sharePrice: 0 },
      bull: { revenue: 0, sharePrice: 0 },
    },
  },
}

const StrictWrapper = ({ children }: { children: ReactNode }) =>
  createElement(StrictMode, null, children)

describe("useProjections", () => {
  beforeEach(() => {
    generateProjectionResultsMock.mockReset()
    getFundamentalDataMock.mockReset()
    generateProjectionResultsMock.mockResolvedValue(sampleBundle)
  })

  // The dedup check: a single render under StrictMode (which double-mounts
  // in dev) must collapse to exactly one bundled fetch and zero standalone
  // fundamentals fetches. Splitting these doubled the daily AV quota burn.
  it("issues exactly one generateProjectionResults call under StrictMode and never calls getFundamentalData", async () => {
    const { result } = renderHook(() => useProjections("AAPL"), {
      wrapper: StrictWrapper,
    })

    await waitFor(() => {
      expect(result.current.results).not.toBeNull()
      expect(result.current.fundamentalData).not.toBeNull()
    })

    expect(generateProjectionResultsMock).toHaveBeenCalledTimes(1)
    expect(generateProjectionResultsMock).toHaveBeenCalledWith("AAPL", undefined)
    expect(getFundamentalDataMock).not.toHaveBeenCalled()
  })

  it("unwraps the bundle into separate fundamentals + results state slots", async () => {
    const { result } = renderHook(() => useProjections("AAPL"))
    await waitFor(() => {
      expect(result.current.fundamentalData?.symbol).toBe("AAPL")
      expect(result.current.results?.baseline.year).toBe(2024)
    })
  })

  it("clears state when symbol becomes null", async () => {
    const { result, rerender } = renderHook(
      ({ symbol }: { symbol: string | null }) => useProjections(symbol),
      { initialProps: { symbol: "AAPL" as string | null } },
    )
    await waitFor(() => expect(result.current.results).not.toBeNull())

    rerender({ symbol: null })
    expect(result.current.results).toBeNull()
    expect(result.current.fundamentalData).toBeNull()
  })
})
