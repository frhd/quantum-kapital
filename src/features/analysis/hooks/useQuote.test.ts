import { describe, it, expect, vi, beforeEach, afterEach } from "vitest"
import { renderHook, waitFor, act } from "@testing-library/react"
import { useQuote } from "./useQuote"

const getQuoteMock = vi.fn()
const getDataTierMock = vi.fn()
const listenMock = vi.fn()

vi.mock("../../../shared/api/ibkr", () => ({
  ibkrApi: {
    getQuote: (symbol: string) => getQuoteMock(symbol),
    getDataTier: () => getDataTierMock(),
  },
}))

vi.mock("@tauri-apps/api/event", () => ({
  listen: (event: string, handler: unknown) => listenMock(event, handler),
}))

const sampleQuote = {
  symbol: "AAPL",
  lastPrice: 202.49,
  prevClose: 200.0,
  volume: 1_234_567,
  timestamp: 1_730_000_000,
}

describe("useQuote", () => {
  beforeEach(() => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    getQuoteMock.mockReset()
    getDataTierMock.mockReset()
    listenMock.mockReset()
    listenMock.mockResolvedValue(() => {})
    getQuoteMock.mockResolvedValue(sampleQuote)
    // Existing cadence tests assume real-time (5s) — make that the
    // default. Tier-specific tests below override.
    getDataTierMock.mockResolvedValue("real_time")
    Object.defineProperty(document, "visibilityState", {
      configurable: true,
      get: () => "visible",
    })
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it("fetches immediately on mount", async () => {
    const { result } = renderHook(() => useQuote("AAPL"))

    await waitFor(() => {
      expect(getQuoteMock).toHaveBeenCalledWith("AAPL")
    })

    await waitFor(() => {
      expect(result.current.quote).toEqual(sampleQuote)
    })
  })

  it("polls every 5s while visible and connected", async () => {
    renderHook(() => useQuote("AAPL"))

    await waitFor(() => expect(getQuoteMock).toHaveBeenCalledTimes(1))

    await act(async () => {
      vi.advanceTimersByTime(5_000)
    })
    expect(getQuoteMock).toHaveBeenCalledTimes(2)

    await act(async () => {
      vi.advanceTimersByTime(5_000)
    })
    expect(getQuoteMock).toHaveBeenCalledTimes(3)
  })

  it("does not poll when symbol is null", async () => {
    renderHook(() => useQuote(null))
    await act(async () => {
      vi.advanceTimersByTime(15_000)
    })
    expect(getQuoteMock).not.toHaveBeenCalled()
  })

  it("preserves last good quote across transient errors", async () => {
    getQuoteMock
      .mockResolvedValueOnce(sampleQuote)
      .mockRejectedValueOnce("timeout")
      .mockResolvedValueOnce({ ...sampleQuote, lastPrice: 203.1 })

    const { result } = renderHook(() => useQuote("AAPL"))

    await waitFor(() => expect(result.current.quote?.lastPrice).toBe(202.49))

    await act(async () => {
      vi.advanceTimersByTime(5_000)
    })
    await waitFor(() => expect(result.current.error).toBe("fetch_failed"))
    // last good quote unchanged
    expect(result.current.quote?.lastPrice).toBe(202.49)

    await act(async () => {
      vi.advanceTimersByTime(5_000)
    })
    await waitFor(() => expect(result.current.quote?.lastPrice).toBe(203.1))
    expect(result.current.error).toBeNull()
  })

  it("maps backend error discriminants to QuoteError values", async () => {
    getQuoteMock.mockRejectedValueOnce("disconnected")
    const { result } = renderHook(() => useQuote("AAPL"))
    await waitFor(() => expect(result.current.error).toBe("disconnected"))

    getQuoteMock.mockReset()
    getQuoteMock.mockRejectedValueOnce("no_permission")
    const { result: result2 } = renderHook(() => useQuote("MSFT"))
    await waitFor(() => expect(result2.current.error).toBe("no_permission"))
  })

  it("subscribes to connection-status-changed and stops polling on disconnect", async () => {
    let connectionHandler: ((event: { payload: unknown }) => void) | null = null
    listenMock.mockImplementation((eventName, handler) => {
      if (eventName === "connection-status-changed") {
        connectionHandler = handler as (event: { payload: unknown }) => void
      }
      return Promise.resolve(() => {})
    })

    renderHook(() => useQuote("AAPL"))

    await waitFor(() => expect(getQuoteMock).toHaveBeenCalledTimes(1))

    // Simulate disconnect event
    act(() => {
      connectionHandler?.({
        payload: { type: "ConnectionStatusChanged", data: { connected: false, message: "down" } },
      })
    })

    await act(async () => {
      vi.advanceTimersByTime(15_000)
    })
    expect(getQuoteMock).toHaveBeenCalledTimes(1) // no new calls

    // Reconnect
    act(() => {
      connectionHandler?.({
        payload: { type: "ConnectionStatusChanged", data: { connected: true, message: "up" } },
      })
    })

    await waitFor(() => expect(getQuoteMock).toHaveBeenCalledTimes(2))
  })

  it("polls every 60s when data-tier-detected emits Delayed", async () => {
    let tierHandler: ((event: { payload: unknown }) => void) | null = null
    listenMock.mockImplementation((eventName, handler) => {
      if (eventName === "data-tier-detected") {
        tierHandler = handler as (event: { payload: unknown }) => void
      }
      return Promise.resolve(() => {})
    })
    // Suppress the mount hydration so the tier change comes from the event.
    getDataTierMock.mockResolvedValue("unknown")

    renderHook(() => useQuote("AAPL"))
    await waitFor(() => expect(getQuoteMock).toHaveBeenCalledTimes(1))

    act(() => {
      tierHandler?.({
        payload: { type: "DataTierDetected", data: { tier: "delayed" } },
      })
    })

    // No tick at 5s under delayed cadence.
    await act(async () => {
      vi.advanceTimersByTime(5_000)
    })
    expect(getQuoteMock).toHaveBeenCalledTimes(1)

    // 60s elapses → one delayed tick.
    await act(async () => {
      vi.advanceTimersByTime(55_000)
    })
    await waitFor(() => expect(getQuoteMock).toHaveBeenCalledTimes(2))
  })

  it("polls every 5s when data-tier-detected emits RealTime", async () => {
    let tierHandler: ((event: { payload: unknown }) => void) | null = null
    listenMock.mockImplementation((eventName, handler) => {
      if (eventName === "data-tier-detected") {
        tierHandler = handler as (event: { payload: unknown }) => void
      }
      return Promise.resolve(() => {})
    })
    getDataTierMock.mockResolvedValue("unknown")

    renderHook(() => useQuote("AAPL"))
    await waitFor(() => expect(getQuoteMock).toHaveBeenCalledTimes(1))

    act(() => {
      tierHandler?.({
        payload: { type: "DataTierDetected", data: { tier: "real_time" } },
      })
    })

    await act(async () => {
      vi.advanceTimersByTime(5_000)
    })
    await waitFor(() => expect(getQuoteMock).toHaveBeenCalledTimes(2))

    await act(async () => {
      vi.advanceTimersByTime(5_000)
    })
    await waitFor(() => expect(getQuoteMock).toHaveBeenCalledTimes(3))
  })

  it("does not poll when tier is Unknown", async () => {
    getDataTierMock.mockResolvedValue("unknown")

    renderHook(() => useQuote("AAPL"))
    // Mount fetch always fires once before the tier guard kicks in.
    await waitFor(() => expect(getQuoteMock).toHaveBeenCalledTimes(1))

    await act(async () => {
      vi.advanceTimersByTime(120_000)
    })
    expect(getQuoteMock).toHaveBeenCalledTimes(1)
  })

  it("pauses polling when the tab is hidden and resumes when visible", async () => {
    renderHook(() => useQuote("AAPL"))
    await waitFor(() => expect(getQuoteMock).toHaveBeenCalledTimes(1))

    Object.defineProperty(document, "visibilityState", {
      configurable: true,
      get: () => "hidden",
    })
    act(() => {
      document.dispatchEvent(new Event("visibilitychange"))
    })

    await act(async () => {
      vi.advanceTimersByTime(15_000)
    })
    expect(getQuoteMock).toHaveBeenCalledTimes(1)

    Object.defineProperty(document, "visibilityState", {
      configurable: true,
      get: () => "visible",
    })
    act(() => {
      document.dispatchEvent(new Event("visibilitychange"))
    })

    await waitFor(() => expect(getQuoteMock).toHaveBeenCalledTimes(2))
  })
})
