import { describe, it, expect, vi, beforeEach } from "vitest"
import { render, screen, waitFor } from "@testing-library/react"
import { useEffect } from "react"
import { NewsPanel } from "./NewsPanel"
import { WorkspaceProvider, useWorkspace } from "../../context/WorkspaceContext"
import type { CachedTickerNews } from "../../types"

const useTickerNewsMock = vi.fn()

vi.mock("../../hooks/useTickerNews", () => ({
  useTickerNews: (symbol: string | null) => useTickerNewsMock(symbol),
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
      <NewsPanel />
    </WorkspaceProvider>,
  )
}

const baseHookReturn = {
  data: null as CachedTickerNews | null,
  loading: false,
  error: null as string | null,
  verdict: null,
  refresh: vi.fn(),
}

const sampleNews: CachedTickerNews = {
  symbol: "AAPL",
  fetched_at_unix: Math.floor(Date.now() / 1000) - 600, // 10m ago
  verdict_json: null,
  items: [
    {
      time_published: "2026-05-01T12:00:00Z",
      title: "Apple beats earnings",
      summary: "Q4 numbers above consensus.",
      source: "Reuters",
      url: "https://example.com/aapl-q4",
      overall_sentiment_score: 0.45,
      overall_sentiment_label: "Bullish",
      ticker_sentiment: [],
    },
  ],
}

describe("NewsPanel", () => {
  beforeEach(() => {
    useTickerNewsMock.mockReset()
    useTickerNewsMock.mockReturnValue({ ...baseHookReturn })
  })

  it("accepts no props — reads from workspace context only", () => {
    expect(NewsPanel.length).toBe(0)
  })

  it("shows the no-symbol empty state when workspace has no active symbol", () => {
    renderPanel(null)
    expect(screen.getByText(/No symbol selected/)).toBeInTheDocument()
  })

  it("passes the active symbol to useTickerNews", async () => {
    renderPanel("MSFT")
    await waitFor(() => {
      expect(useTickerNewsMock).toHaveBeenCalled()
    })
    const lastCall = useTickerNewsMock.mock.calls[useTickerNewsMock.mock.calls.length - 1]
    expect(lastCall?.[0]).toBe("MSFT")
  })

  it("renders cached news items with verdict-pending chip when no verdict_json", async () => {
    useTickerNewsMock.mockReturnValue({
      ...baseHookReturn,
      data: sampleNews,
    })
    renderPanel("AAPL")
    await waitFor(() => {
      expect(screen.getByText("Apple beats earnings")).toBeInTheDocument()
    })
    expect(screen.getByText(/Verdict pending/)).toBeInTheDocument()
    expect(screen.getByRole("link", { name: /open/i })).toHaveAttribute(
      "href",
      "https://example.com/aapl-q4",
    )
  })

  it("renders the structured verdict chip when verdict is present", async () => {
    useTickerNewsMock.mockReturnValue({
      ...baseHookReturn,
      data: {
        ...sampleNews,
        verdict_json: '{"tone":"bullish","ep_worthy":true,"summary":"earnings beat"}',
      },
      verdict: { tone: "bullish", ep_worthy: true, summary: "earnings beat" },
    })
    renderPanel("AAPL")
    await waitFor(() => {
      expect(screen.getByText("bullish")).toBeInTheDocument()
    })
    expect(screen.getByText("EP-worthy")).toBeInTheDocument()
    expect(screen.getByText("earnings beat")).toBeInTheDocument()
  })

  it("renders the no-cache-row empty state when fetched_at_unix is 0", async () => {
    useTickerNewsMock.mockReturnValue({
      ...baseHookReturn,
      data: { symbol: "AAPL", items: [], verdict_json: null, fetched_at_unix: 0 },
    })
    renderPanel("AAPL")
    await waitFor(() => {
      expect(screen.getByText(/No cached news for AAPL/)).toBeInTheDocument()
    })
  })

  it("renders the empty-items state when row exists but items are empty", async () => {
    useTickerNewsMock.mockReturnValue({
      ...baseHookReturn,
      data: {
        symbol: "AAPL",
        items: [],
        verdict_json: null,
        fetched_at_unix: Math.floor(Date.now() / 1000) - 60,
      },
    })
    renderPanel("AAPL")
    await waitFor(() => {
      expect(screen.getByText(/No news items in cache for AAPL/)).toBeInTheDocument()
    })
  })

  it("renders the error state when useTickerNews surfaces an error", async () => {
    useTickerNewsMock.mockReturnValue({ ...baseHookReturn, error: "boom" })
    renderPanel("AAPL")
    await waitFor(() => {
      expect(screen.getByText(/Failed to load news/)).toBeInTheDocument()
    })
    expect(screen.getByText("boom")).toBeInTheDocument()
  })

  it("renders a loading state while data is null (loading flag true or false)", () => {
    useTickerNewsMock.mockReturnValue({ ...baseHookReturn, loading: true })
    renderPanel("AAPL")
    expect(screen.getByText(/Loading news/)).toBeInTheDocument()
  })
})
