import { useCallback, useEffect, useRef, useState } from "react"
import { Card, CardContent } from "../shared/components/ui/card"
import { AlertCircle } from "lucide-react"

import { AppLayout } from "../shared/components/layout/AppLayout"
import type { PageId } from "../shared/components/layout/Sidebar"
import { ConnectionSettings } from "../features/connection/components/ConnectionSettings"
import { AccountSummary } from "../features/portfolio/components/AccountSummary"
import { StockPositions } from "../features/portfolio/components/StockPositions"
import { OptionPositions } from "../features/portfolio/components/OptionPositions"
import { AccountDetails } from "../features/portfolio/components/AccountDetails"
import { TickerAnalysis } from "../features/analysis/components/TickerAnalysis"
import { MarketScanner } from "../features/scanner/components/MarketScanner"
import { CandidateBrowser } from "../features/candidates/components/CandidateBrowser"
import { TrackerTab } from "../features/tracker/components/TrackerTab"
import { AddToTrackerDialog } from "../features/tracker/components/AddToTrackerDialog"
import { ResearchTab } from "../features/research/components/ResearchTab"
import { EvalTab } from "../features/eval/components/EvalTab"
import { DataTierBanner } from "../shared/components/DataTierBanner"
import type { AddToTrackerPrefill } from "../features/tracker/types"

import { useConnection } from "../features/connection/hooks/useConnection"
import { useAccountData } from "../features/portfolio/hooks/useAccountData"

export default function App() {
  const [currentPage, setCurrentPage] = useState<PageId>("analysis")
  const nonceRef = useRef(0)
  const [pendingAnalysisSymbol, setPendingAnalysisSymbol] = useState<{
    symbol: string
    nonce: number
  } | null>(null)
  const [trackerVersion, setTrackerVersion] = useState(0)
  const [trackerCount, setTrackerCount] = useState(0)
  const [addDialogOpen, setAddDialogOpen] = useState(false)
  const [addDialogPrefill, setAddDialogPrefill] = useState<AddToTrackerPrefill | null>(null)

  const handleSelectSymbol = useCallback((symbol: string) => {
    nonceRef.current += 1
    setPendingAnalysisSymbol({ symbol, nonce: nonceRef.current })
    setCurrentPage("analysis")
  }, [])

  const handleRequestAddToTracker = useCallback((prefill: AddToTrackerPrefill) => {
    setAddDialogPrefill(prefill)
    setAddDialogOpen(true)
  }, [])

  const handleAddTrackerManual = useCallback(() => {
    setAddDialogPrefill({ symbol: "", source: "manual" })
    setAddDialogOpen(true)
  }, [])

  const handleDialogClose = useCallback(() => {
    setAddDialogOpen(false)
  }, [])

  const handleDialogAdded = useCallback(() => {
    setTrackerVersion((v) => v + 1)
  }, [])

  const {
    connectionStatus,
    connectionSettings,
    setConnectionSettings,
    loading,
    disconnecting,
    error,
    connect,
    disconnect,
  } = useConnection()

  const { accounts, accountSummary, positions, fetchAccountData, clearAccountData } =
    useAccountData()

  useEffect(() => {
    if (connectionStatus.connected && accounts.length === 0) {
      console.log("Connection detected on mount, fetching account data")
      fetchAccountData()
    }
  }, [connectionStatus.connected, accounts.length, fetchAccountData])

  const handleConnect = async () => {
    try {
      await connect()
    } catch {
      // handled in hook
    }
  }

  const handleDisconnect = async () => {
    try {
      await disconnect()
      clearAccountData()
    } catch (err) {
      console.error("disconnect error:", err)
    }
  }

  return (
    <AppLayout
      currentPage={currentPage}
      onNavigate={setCurrentPage}
      connectionStatus={connectionStatus}
      loading={loading}
      disconnecting={disconnecting}
      onConnect={handleConnect}
      onDisconnect={handleDisconnect}
      badges={{ tracker: trackerCount }}
    >
      <div className="space-y-6">
        {error && (
          <Card className="border-destructive/50 bg-destructive/10">
            <CardContent className="flex items-center gap-2 p-4">
              <AlertCircle className="text-destructive h-5 w-5" />
              <p className="text-destructive">{error}</p>
            </CardContent>
          </Card>
        )}

        {!connectionStatus.connected && (
          <ConnectionSettings
            connectionSettings={connectionSettings}
            setConnectionSettings={setConnectionSettings}
          />
        )}

        {connectionStatus.connected && (
          <>
            <DataTierBanner />

            <AccountSummary
              accounts={accounts}
              accountSummary={accountSummary}
              positions={positions}
            />

            {currentPage === "analysis" && <TickerAnalysis pendingSymbol={pendingAnalysisSymbol} />}

            {currentPage === "scanner" && (
              <MarketScanner
                onSelectSymbol={handleSelectSymbol}
                onAddToTracker={handleRequestAddToTracker}
              />
            )}

            {currentPage === "candidates" && <CandidateBrowser />}

            {currentPage === "tracker" && (
              <TrackerTab
                refreshKey={trackerVersion}
                onSelectSymbol={handleSelectSymbol}
                onAddClick={handleAddTrackerManual}
                onCountChange={setTrackerCount}
              />
            )}

            {currentPage === "research" && <ResearchTab />}

            {currentPage === "eval" && <EvalTab />}

            {currentPage === "positions" && (
              <>
                <StockPositions positions={positions} />
                <OptionPositions positions={positions} />
                {positions.length === 0 && (
                  <Card>
                    <CardContent className="py-8 text-center">
                      <p className="text-muted-foreground">No positions found</p>
                    </CardContent>
                  </Card>
                )}
              </>
            )}

            {currentPage === "account" && (
              <AccountDetails
                accounts={accounts}
                accountSummary={accountSummary}
                connectionStatus={connectionStatus}
              />
            )}
          </>
        )}
      </div>

      <AddToTrackerDialog
        open={addDialogOpen}
        prefill={addDialogPrefill}
        onClose={handleDialogClose}
        onAdded={handleDialogAdded}
      />
    </AppLayout>
  )
}
