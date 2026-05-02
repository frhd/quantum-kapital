import { useState, useEffect } from "react"
import { ibkrApi } from "../../../shared/api/ibkr"
import type {
  ProjectionResults,
  ProjectionAssumptions,
  FundamentalData,
} from "../../../shared/types"

export function useProjections(symbol: string | null, assumptions?: ProjectionAssumptions) {
  const [results, setResults] = useState<ProjectionResults | null>(null)
  const [fundamentalData, setFundamentalData] = useState<FundamentalData | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (!symbol) {
      setResults(null)
      setFundamentalData(null)
      return
    }

    const fetchProjections = async () => {
      setLoading(true)
      setError(null)

      try {
        // Single Tauri call returns fundamentals + projection results.
        // Splitting this into a parallel `getFundamentalData` doubled
        // the daily AV quota burn (3 endpoints × 2 calls).
        const bundle = await ibkrApi.generateProjectionResults(symbol, assumptions)
        setFundamentalData(bundle.fundamentals)
        setResults(bundle.results)
      } catch (err) {
        setError(err instanceof Error ? err.message : "Failed to fetch projections")
        console.error("Error fetching projections:", err)
      } finally {
        setLoading(false)
      }
    }

    fetchProjections()
  }, [symbol, assumptions])

  return { results, fundamentalData, loading, error }
}
