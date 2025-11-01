import { useState, useEffect } from "react"
import { ibkrApi } from "../../../shared/api/ibkr"
import type { ScenarioProjections, ProjectionAssumptions } from "../../../shared/types"

export function useProjections(symbol: string | null, assumptions?: ProjectionAssumptions) {
  const [projections, setProjections] = useState<ScenarioProjections | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (!symbol) {
      setProjections(null)
      return
    }

    const fetchProjections = async () => {
      setLoading(true)
      setError(null)

      try {
        const data = await ibkrApi.generateProjections(symbol, assumptions)
        setProjections(data)
      } catch (err) {
        setError(err instanceof Error ? err.message : "Failed to fetch projections")
        console.error("Error fetching projections:", err)
      } finally {
        setLoading(false)
      }
    }

    fetchProjections()
  }, [symbol, assumptions])

  return { projections, loading, error }
}
