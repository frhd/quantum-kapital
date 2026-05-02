import { describe, it, expect, vi, beforeEach } from "vitest"
import { renderHook, waitFor } from "@testing-library/react"
import { useTickerNews } from "./useTickerNews"
import type { CachedTickerNews } from "../types"

const getCachedNewsMock = vi.fn()

vi.mock("../../../shared/api/ibkr", () => ({
  ibkrApi: {
    tracker: {
      getCachedNews: (symbol: string) => getCachedNewsMock(symbol),
    },
  },
}))

const samplePayload: CachedTickerNews = {
  symbol: "AAPL",
  fetched_at_unix: 100,
  verdict_json: '{"tone":"bullish","ep_worthy":true,"summary":"earnings beat"}',
  items: [],
}

describe("useTickerNews", () => {
  beforeEach(() => {
    getCachedNewsMock.mockReset()
  })

  it("decodes a well-formed verdict_json into a structured verdict", async () => {
    getCachedNewsMock.mockResolvedValue(samplePayload)
    const { result } = renderHook(() => useTickerNews("AAPL"))
    await waitFor(() => {
      expect(result.current.loading).toBe(false)
    })
    expect(result.current.verdict).toEqual({
      tone: "bullish",
      ep_worthy: true,
      summary: "earnings beat",
    })
  })

  it("returns a null verdict when verdict_json is null", async () => {
    getCachedNewsMock.mockResolvedValue({ ...samplePayload, verdict_json: null })
    const { result } = renderHook(() => useTickerNews("AAPL"))
    await waitFor(() => {
      expect(result.current.loading).toBe(false)
    })
    expect(result.current.verdict).toBeNull()
  })

  it("falls back to null verdict when verdict_json is malformed", async () => {
    getCachedNewsMock.mockResolvedValue({ ...samplePayload, verdict_json: "not json" })
    const { result } = renderHook(() => useTickerNews("AAPL"))
    await waitFor(() => {
      expect(result.current.loading).toBe(false)
    })
    expect(result.current.verdict).toBeNull()
  })

  it("does not call the API when symbol is null", async () => {
    const { result } = renderHook(() => useTickerNews(null))
    await waitFor(() => {
      expect(result.current.loading).toBe(false)
    })
    expect(getCachedNewsMock).not.toHaveBeenCalled()
    expect(result.current.data).toBeNull()
  })

  it("surfaces errors as a string", async () => {
    getCachedNewsMock.mockRejectedValue(new Error("boom"))
    const { result } = renderHook(() => useTickerNews("AAPL"))
    await waitFor(() => {
      expect(result.current.error).toBe("boom")
    })
    expect(result.current.data).toBeNull()
  })
})
