import { useState, useEffect } from "react"
import { ibkrApi } from "../../../shared/api/ibkr"
import type { ScenarioProjections, ProjectionAssumptions, FundamentalData } from "../../../shared/types"

export function useProjections(symbol: string | null, assumptions?: ProjectionAssumptions) {
  const [projections, setProjections] = useState<ScenarioProjections | null>(null)
  const [fundamentalData, setFundamentalData] = useState<FundamentalData | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (!symbol) {
      setProjections(null)
      setFundamentalData(null)
      return
    }

    const fetchProjections = async () => {
      setLoading(true)
      setError(null)

      try {
        // Fetch both fundamental data and projections
        const [fundamentals, projectionsData] = await Promise.all([
          ibkrApi.getFundamentalData(symbol),
          ibkrApi.generateProjections(symbol, assumptions)
        ])

        setFundamentalData(fundamentals)
        setProjections(projectionsData)
      } catch (err) {
        setError(err instanceof Error ? err.message : "Failed to fetch projections")
        console.error("Error fetching projections:", err)
      } finally {
        setLoading(false)
      }
    }

    fetchProjections()
  }, [symbol, assumptions])

  return { projections, fundamentalData, loading, error }
}
