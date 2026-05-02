import { useState } from "react"
import { ScannerFilters } from "./ScannerFilters"
import { ScannerResults } from "./ScannerResults"
import { useScanner } from "../hooks/useScanner"
import type { ScannerData, ScannerSubscription } from "../../../shared/types"
import type { AddToTrackerPrefill } from "../../tracker/types"

interface MarketScannerProps {
  onAddToTracker: (prefill: AddToTrackerPrefill) => void
}

export function MarketScanner({ onAddToTracker }: MarketScannerProps) {
  const [subscription, setSubscription] = useState<ScannerSubscription | null>(null)
  const { results, lastUpdate, error } = useScanner(subscription)

  const isRunning = subscription !== null

  const handleAddToTracker = (row: ScannerData) => {
    onAddToTracker({
      symbol: row.contract.symbol,
      source: "scanner",
      sourceMeta: {
        rank: row.rank,
        scan_code: subscription?.scan_code ?? null,
        exchange: row.contract.primary_exchange || row.contract.exchange,
        contract_id: row.contract.contract_id,
      },
    })
  }

  return (
    <div className="space-y-4">
      <ScannerFilters
        isRunning={isRunning}
        onStart={setSubscription}
        onStop={() => setSubscription(null)}
      />
      <ScannerResults
        results={results}
        lastUpdate={lastUpdate}
        isRunning={isRunning}
        error={error}
        onAddToTracker={handleAddToTracker}
      />
    </div>
  )
}
