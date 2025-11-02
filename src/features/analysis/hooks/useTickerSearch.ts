import { useState, useCallback } from "react"
import { ibkrApi } from "../../../shared/api/ibkr"
import type { TickerSearchResult, TickerData } from "../types"
import type { FundamentalData } from "../../../shared/types"

// Mock ticker search results (for autocomplete)
// In a real implementation, this could be replaced with a proper ticker search API
const mockTickers: TickerSearchResult[] = [
  { symbol: "AAPL", name: "Apple Inc.", exchange: "NASDAQ", type: "Stock" },
  { symbol: "MSFT", name: "Microsoft Corporation", exchange: "NASDAQ", type: "Stock" },
  { symbol: "GOOGL", name: "Alphabet Inc.", exchange: "NASDAQ", type: "Stock" },
  { symbol: "AMZN", name: "Amazon.com Inc.", exchange: "NASDAQ", type: "Stock" },
  { symbol: "TSLA", name: "Tesla Inc.", exchange: "NASDAQ", type: "Stock" },
  { symbol: "META", name: "Meta Platforms Inc.", exchange: "NASDAQ", type: "Stock" },
  { symbol: "NVDA", name: "NVIDIA Corporation", exchange: "NASDAQ", type: "Stock" },
  { symbol: "AMD", name: "Advanced Micro Devices Inc.", exchange: "NASDAQ", type: "Stock" },
]

export function useTickerSearch() {
  const [searchQuery, setSearchQuery] = useState("")
  const [searchResults, setSearchResults] = useState<TickerSearchResult[]>([])
  const [selectedTicker, setSelectedTicker] = useState<TickerData | null>(null)
  const [loading, setLoading] = useState(false)

  const searchTickers = useCallback((query: string) => {
    setSearchQuery(query)

    if (query.length === 0) {
      setSearchResults([])
      return
    }

    // Mock search - filter tickers by symbol or name
    const filtered = mockTickers.filter(
      (ticker) =>
        ticker.symbol.toLowerCase().includes(query.toLowerCase()) ||
        ticker.name.toLowerCase().includes(query.toLowerCase())
    )

    setSearchResults(filtered)
  }, [])

  const selectTicker = useCallback(async (symbol: string) => {
    setLoading(true)

    try {
      // Fetch real fundamental data from the API
      const fundamentalData: FundamentalData = await ibkrApi.getFundamentalData(symbol)

      // Convert FundamentalData to TickerData format
      const tickerData: TickerData = {
        symbol: fundamentalData.symbol,
        name: fundamentalData.currentMetrics.name ||
              mockTickers.find((t) => t.symbol === symbol)?.name ||
              symbol,
        exchange: fundamentalData.currentMetrics.exchange || "NASDAQ",
        type: "Stock",
        price: fundamentalData.currentMetrics.price,
        // Note: Alpha Vantage OVERVIEW doesn't provide change/changePercent or volume
        // These would require a separate real-time quote endpoint
        change: undefined,
        changePercent: undefined,
        volume: undefined,
        marketCap: fundamentalData.currentMetrics.marketCap || undefined,
        pe: fundamentalData.currentMetrics.peRatio,
        yield: fundamentalData.currentMetrics.dividendYield,
      }

      setSelectedTicker(tickerData)
      setSearchQuery(symbol)
      setSearchResults([])
    } catch (error) {
      console.error("Error fetching ticker data:", error)

      // Fallback to basic data if API fails
      const fallbackData: TickerData = {
        symbol,
        name: mockTickers.find((t) => t.symbol === symbol)?.name || symbol,
        exchange: mockTickers.find((t) => t.symbol === symbol)?.exchange || "NASDAQ",
        type: mockTickers.find((t) => t.symbol === symbol)?.type || "Stock",
      }
      setSelectedTicker(fallbackData)
      setSearchQuery(symbol)
      setSearchResults([])
    } finally {
      setLoading(false)
    }
  }, [])

  const clearSelection = useCallback(() => {
    setSelectedTicker(null)
    setSearchQuery("")
    setSearchResults([])
  }, [])

  return {
    searchQuery,
    searchResults,
    selectedTicker,
    loading,
    searchTickers,
    selectTicker,
    clearSelection,
  }
}
