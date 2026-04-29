import { useEffect, useRef, useState } from "react"
import { Card, CardContent } from "../shared/components/ui/card"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "../shared/components/ui/tabs"
import { AlertCircle, BarChart3, Settings, LineChart, Search } from "lucide-react"

import { PageHeader } from "../shared/components/layout/PageHeader"
import { ConnectionSettings } from "../features/connection/components/ConnectionSettings"
import { AccountSummary } from "../features/portfolio/components/AccountSummary"
import { StockPositions } from "../features/portfolio/components/StockPositions"
import { OptionPositions } from "../features/portfolio/components/OptionPositions"
import { AccountDetails } from "../features/portfolio/components/AccountDetails"
import { TickerAnalysis } from "../features/analysis/components/TickerAnalysis"
import { MarketScanner } from "../features/scanner/components/MarketScanner"

import { useConnection } from "../features/connection/hooks/useConnection"
import { useAccountData } from "../features/portfolio/hooks/useAccountData"

export default function App() {
  const [activeTab, setActiveTab] = useState("analysis")
  const nonceRef = useRef(0)
  const [pendingAnalysisSymbol, setPendingAnalysisSymbol] = useState<{
    symbol: string
    nonce: number
  } | null>(null)

  const handleSelectFromScanner = (symbol: string) => {
    nonceRef.current += 1
    setPendingAnalysisSymbol({ symbol, nonce: nonceRef.current })
    setActiveTab("analysis")
  }

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

  // Fetch account data when connection is detected on mount
  useEffect(() => {
    if (connectionStatus.connected && accounts.length === 0) {
      console.log("Connection detected on mount, fetching account data")
      fetchAccountData()
    }
  }, [connectionStatus.connected, accounts.length, fetchAccountData])

  const handleConnect = async () => {
    try {
      await connect()
      // The useEffect above watches connectionStatus.connected and runs
      // fetchAccountData(); calling it here too caused parallel calls that
      // raced on ibapi's shared account_updates channel, splitting positions.
    } catch {
      // Error is already handled in the hook
    }
  }

  const handleDisconnect = async () => {
    console.log("🔴 APP: handleDisconnect called")
    try {
      await disconnect()
      console.log("🔴 APP: disconnect() completed")
      clearAccountData()
      console.log("🔴 APP: clearAccountData() completed")
    } catch (err) {
      console.error("🔴 APP: handleDisconnect error:", err)
    }
  }

  return (
    <div className="min-h-screen space-y-6 bg-slate-900 p-4 text-white">
      {/* Header */}
      <PageHeader
        connectionStatus={connectionStatus}
        loading={loading}
        disconnecting={disconnecting}
        onConnect={handleConnect}
        onDisconnect={handleDisconnect}
      />

      {/* Error Alert */}
      {error && (
        <Card className="border-red-800 bg-red-900/20">
          <CardContent className="flex items-center gap-2 p-4">
            <AlertCircle className="h-5 w-5 text-red-400" />
            <p className="text-red-400">{error}</p>
          </CardContent>
        </Card>
      )}

      {/* Connection Settings */}
      {!connectionStatus.connected && (
        <ConnectionSettings
          connectionSettings={connectionSettings}
          setConnectionSettings={setConnectionSettings}
        />
      )}

      {/* Account Summary */}
      {connectionStatus.connected && (
        <>
          <AccountSummary
            accounts={accounts}
            accountSummary={accountSummary}
            positions={positions}
          />

          {/* Main Content Tabs */}
          <Tabs value={activeTab} onValueChange={setActiveTab} className="space-y-4">
            <TabsList className="border border-slate-700 bg-slate-800/50">
              <TabsTrigger
                value="analysis"
                className="data-[state=active]:bg-slate-700 data-[state=active]:text-white"
              >
                <LineChart className="mr-2 h-4 w-4" />
                Analysis
              </TabsTrigger>
              <TabsTrigger
                value="positions"
                className="data-[state=active]:bg-slate-700 data-[state=active]:text-white"
              >
                <BarChart3 className="mr-2 h-4 w-4" />
                Positions
              </TabsTrigger>
              <TabsTrigger
                value="account"
                className="data-[state=active]:bg-slate-700 data-[state=active]:text-white"
              >
                <Settings className="mr-2 h-4 w-4" />
                Account Details
              </TabsTrigger>
              <TabsTrigger
                value="scanner"
                className="data-[state=active]:bg-slate-700 data-[state=active]:text-white"
              >
                <Search className="mr-2 h-4 w-4" />
                Scanner
              </TabsTrigger>
            </TabsList>

            <TabsContent value="analysis" className="space-y-4">
              <TickerAnalysis pendingSymbol={pendingAnalysisSymbol} />
            </TabsContent>

            <TabsContent value="positions" className="space-y-4">
              <StockPositions positions={positions} />
              <OptionPositions positions={positions} />

              {positions.length === 0 && (
                <Card className="border-slate-700 bg-slate-800/50 backdrop-blur-xs">
                  <CardContent className="py-8 text-center">
                    <p className="text-slate-400">No positions found</p>
                  </CardContent>
                </Card>
              )}
            </TabsContent>

            <TabsContent value="account" className="space-y-4">
              <AccountDetails
                accounts={accounts}
                accountSummary={accountSummary}
                connectionStatus={connectionStatus}
              />
            </TabsContent>

            <TabsContent value="scanner" className="space-y-4">
              <MarketScanner onSelectSymbol={handleSelectFromScanner} />
            </TabsContent>
          </Tabs>
        </>
      )}
    </div>
  )
}
