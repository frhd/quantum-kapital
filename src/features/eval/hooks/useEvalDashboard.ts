import { useCallback, useEffect, useState } from "react"

import { ibkrApi } from "../../../shared/api/ibkr"
import type { CalibrationStats, CostAttribution } from "../types"

interface UseEvalDashboardResult {
  calibration: CalibrationStats | null
  cost: CostAttribution | null
  loading: boolean
  error: string | null
  windowDays: number
  setWindowDays: (n: number) => void
  refresh: () => Promise<void>
}

const DEFAULT_WINDOW = 30

/**
 * Phase 8 — pulls the two summary roll-ups for the eval dashboard.
 *
 * Refreshes on mount + every `windowDays` change. The two queries share
 * the same window so the dashboard's headline numbers (calibration vs.
 * cost) always describe the same time slice.
 */
export function useEvalDashboard(): UseEvalDashboardResult {
  const [calibration, setCalibration] = useState<CalibrationStats | null>(null)
  const [cost, setCost] = useState<CostAttribution | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [windowDays, setWindowDays] = useState<number>(DEFAULT_WINDOW)

  const refresh = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      const [c, k] = await Promise.all([
        ibkrApi.eval.calibrationStats(windowDays),
        ibkrApi.eval.costAttribution(windowDays),
      ])
      setCalibration(c)
      setCost(k)
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setLoading(false)
    }
  }, [windowDays])

  useEffect(() => {
    void refresh()
  }, [refresh])

  return { calibration, cost, loading, error, windowDays, setWindowDays, refresh }
}
