import { useState } from "react"
import { ScannerFilters } from "./ScannerFilters"
import { ScannerResults } from "./ScannerResults"
import { useScanner } from "../hooks/useScanner"
import type { ScannerSubscription } from "../../../shared/types"

interface MarketScannerProps {
  onSelectSymbol: (symbol: string) => void
}

export function MarketScanner({ onSelectSymbol }: MarketScannerProps) {
  const [subscription, setSubscription] = useState<ScannerSubscription | null>(null)
  const { results, lastUpdate, error } = useScanner(subscription)

  const isRunning = subscription !== null

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
        onSelectSymbol={onSelectSymbol}
      />
    </div>
  )
}
