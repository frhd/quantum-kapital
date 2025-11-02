import { useState, useCallback, useEffect } from "react"
import { ibkrApi } from "../../../shared/api/ibkr"
import type { TickerSearchResult, TickerData } from "../types"
import type { FundamentalData } from "../../../shared/types"

export function useTickerSearch() {
  const [searchQuery, setSearchQuery] = useState("")
  const [searchResults, setSearchResults] = useState<TickerSearchResult[]>([])
  const [cachedTickers, setCachedTickers] = useState<TickerSearchResult[]>([])
  const [selectedTicker, setSelectedTicker] = useState<TickerData | null>(null)
  const [loading, setLoading] = useState(false)

  // Fetch cached tickers on mount
  useEffect(() => {
    const loadCachedTickers = async () => {
      try {
        const symbols = await ibkrApi.getCachedTickers()
        const tickers: TickerSearchResult[] = symbols.map((symbol) => ({
          symbol,
          name: symbol, // Name will be populated from API when selected
          exchange: "Unknown",
          type: "Stock",
        }))
        setCachedTickers(tickers)
      } catch (error) {
        console.error("Error loading cached tickers:", error)
        setCachedTickers([])
      }
    }

    loadCachedTickers()
  }, [])

  const searchTickers = useCallback(
    (query: string) => {
      setSearchQuery(query)

      if (query.length === 0) {
        setSearchResults([])
        return
      }

      // Filter cached tickers by symbol
      const filtered = cachedTickers.filter((ticker) =>
        ticker.symbol.toLowerCase().includes(query.toLowerCase())
      )

      setSearchResults(filtered)
    },
    [cachedTickers]
  )

  const selectTicker = useCallback(async (symbol: string) => {
    setLoading(true)

    // Normalize symbol to uppercase
    const normalizedSymbol = symbol.toUpperCase()

    try {
      // Fetch real fundamental data from the API
      const fundamentalData: FundamentalData = await ibkrApi.getFundamentalData(normalizedSymbol)

      // Convert FundamentalData to TickerData format
      const tickerData: TickerData = {
        symbol: fundamentalData.symbol,
        name: fundamentalData.currentMetrics.name || normalizedSymbol,
        exchange: fundamentalData.currentMetrics.exchange || "Unknown",
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
      setSearchQuery(normalizedSymbol)
      setSearchResults([])
    } catch (error) {
      console.error("Error fetching ticker data:", error)

      // Fallback to basic data if API fails
      const fallbackData: TickerData = {
        symbol: normalizedSymbol,
        name: normalizedSymbol,
        exchange: "Unknown",
        type: "Stock",
      }
      setSelectedTicker(fallbackData)
      setSearchQuery(normalizedSymbol)
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
