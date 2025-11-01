import { Card, CardContent } from "../shared/components/ui/card"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "../shared/components/ui/tabs"
import { AlertCircle, BarChart3, Settings, LineChart } from "lucide-react"

import { PageHeader } from "../shared/components/layout/PageHeader"
import { ConnectionSettings } from "../features/connection/components/ConnectionSettings"
import { AccountSummary } from "../features/portfolio/components/AccountSummary"
import { StockPositions } from "../features/portfolio/components/StockPositions"
import { OptionPositions } from "../features/portfolio/components/OptionPositions"
import { AccountDetails } from "../features/portfolio/components/AccountDetails"
import { TickerAnalysis } from "../features/analysis/components/TickerAnalysis"

import { useConnection } from "../features/connection/hooks/useConnection"
import { useAccountData } from "../features/portfolio/hooks/useAccountData"

export default function App() {
  const {
    connectionStatus,
    connectionSettings,
    setConnectionSettings,
    loading,
    error,
    connect,
    disconnect,
  } = useConnection()

  const {
    accounts,
    accountSummary,
    positions,
    fetchAccountData,
    clearAccountData,
  } = useAccountData()

  const handleConnect = async () => {
    try {
      const status = await connect()
      if (status.connected) {
        await fetchAccountData()
      }
    } catch (err) {
      // Error is already handled in the hook
    }
  }

  const handleDisconnect = async () => {
    await disconnect()
    clearAccountData()
  }

  return (
    <div className="min-h-screen bg-slate-900 text-white p-4 space-y-6">
      {/* Header */}
      <PageHeader
        connectionStatus={connectionStatus}
        loading={loading}
        onConnect={handleConnect}
        onDisconnect={handleDisconnect}
      />

      {/* Error Alert */}
      {error && (
        <Card className="bg-red-900/20 border-red-800">
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
          <Tabs defaultValue="analysis" className="space-y-4">
            <TabsList className="bg-slate-800/50 border border-slate-700">
              <TabsTrigger
                value="analysis"
                className="data-[state=active]:bg-slate-700 data-[state=active]:text-white"
              >
                <LineChart className="h-4 w-4 mr-2" />
                Analysis
              </TabsTrigger>
              <TabsTrigger
                value="positions"
                className="data-[state=active]:bg-slate-700 data-[state=active]:text-white"
              >
                <BarChart3 className="h-4 w-4 mr-2" />
                Positions
              </TabsTrigger>
              <TabsTrigger value="account" className="data-[state=active]:bg-slate-700 data-[state=active]:text-white">
                <Settings className="h-4 w-4 mr-2" />
                Account Details
              </TabsTrigger>
            </TabsList>

            <TabsContent value="analysis" className="space-y-4">
              <TickerAnalysis />
            </TabsContent>

            <TabsContent value="positions" className="space-y-4">
              <StockPositions positions={positions} />
              <OptionPositions positions={positions} />

              {positions.length === 0 && (
                <Card className="bg-slate-800/50 border-slate-700 backdrop-blur-sm">
                  <CardContent className="text-center py-8">
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
          </Tabs>
        </>
      )}
    </div>
  )
}