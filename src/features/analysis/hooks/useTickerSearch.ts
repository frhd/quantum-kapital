import { useState, useCallback } from "react"
import type { TickerSearchResult, TickerData } from "../types"

// Mock ticker data for demonstration
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

// Mock detailed ticker data
const mockTickerDetails: Record<string, TickerData> = {
  AAPL: {
    symbol: "AAPL",
    name: "Apple Inc.",
    exchange: "NASDAQ",
    type: "Stock",
    price: 178.25,
    change: 2.15,
    changePercent: 1.22,
    volume: 52430000,
    marketCap: "2.8T",
    pe: 29.5,
    yield: 0.52,
  },
  MSFT: {
    symbol: "MSFT",
    name: "Microsoft Corporation",
    exchange: "NASDAQ",
    type: "Stock",
    price: 378.91,
    change: -1.25,
    changePercent: -0.33,
    volume: 28340000,
    marketCap: "2.8T",
    pe: 35.2,
    yield: 0.78,
  },
  // Add more as needed
}

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

  const selectTicker = useCallback((symbol: string) => {
    setLoading(true)

    // Mock API call - in real implementation, this would fetch from IBKR or another data source
    setTimeout(() => {
      const tickerData = mockTickerDetails[symbol] || {
        symbol,
        name: mockTickers.find((t) => t.symbol === symbol)?.name || symbol,
        exchange: mockTickers.find((t) => t.symbol === symbol)?.exchange || "NASDAQ",
        type: mockTickers.find((t) => t.symbol === symbol)?.type || "Stock",
      }

      setSelectedTicker(tickerData)
      setSearchQuery(symbol)
      setSearchResults([])
      setLoading(false)
    }, 300)
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
