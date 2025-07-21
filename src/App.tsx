import { useState } from "react"
import { invoke } from "@tauri-apps/api/core"
import { Button } from "./components/ui/button"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "./components/ui/card"
import { Input } from "./components/ui/input"
import { Label } from "./components/ui/label"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "./components/ui/tabs"
import { Badge } from "./components/ui/badge"
import {
  Activity,
  AlertCircle,
  CheckCircle,
  DollarSign,
  PieChart,
  Settings,
  TrendingDown,
  TrendingUp,
  Wifi,
  WifiOff,
  BarChart3,
  Target,
  Clock,
} from "lucide-react"

// Types for IBKR data
interface ConnectionConfig {
  host: string
  port: number
  client_id: number
}

interface ConnectionStatus {
  connected: boolean
  server_time: string | null
  client_id: number
}

interface AccountSummary {
  account: string
  tag: string
  value: string
  currency: string
}

interface Position {
  account: string
  symbol: string
  position: number
  average_cost: number
  market_price: number
  market_value: number
  unrealized_pnl: number
  realized_pnl: number
}

interface OrderRequest {
  symbol: string
  action: "Buy" | "Sell"
  quantity: number
  order_type: "Market" | "Limit" | "Stop" | "StopLimit"
  price?: number
}

export default function App() {
  const [connectionStatus, setConnectionStatus] = useState<ConnectionStatus>({
    connected: false,
    server_time: null,
    client_id: 100,
  })
  const [connectionSettings, setConnectionSettings] = useState<ConnectionConfig>({
    host: "127.0.0.1",
    port: 4002,
    client_id: 100,
  })
  const [accounts, setAccounts] = useState<string[]>([])
  const [accountSummary, setAccountSummary] = useState<AccountSummary[]>([])
  const [positions, setPositions] = useState<Position[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const connectToIBKR = async () => {
    setLoading(true)
    setError(null)
    try {
      await invoke("ibkr_connect", { config: connectionSettings })
      const status = await invoke<ConnectionStatus>("ibkr_get_connection_status")
      setConnectionStatus(status)
      
      if (status.connected) {
        // Fetch initial data
        const accountList = await invoke<string[]>("ibkr_get_accounts")
        setAccounts(accountList)
        
        if (accountList.length > 0) {
          const summary = await invoke<AccountSummary[]>("ibkr_get_account_summary", {
            account: accountList[0],
          })
          setAccountSummary(summary)
          
          const pos = await invoke<Position[]>("ibkr_get_positions")
          setPositions(pos)
        }
      }
    } catch (err) {
      console.error("Failed to connect to IBKR:", err)
      setError(err as string)
    } finally {
      setLoading(false)
    }
  }

  const disconnectFromIBKR = async () => {
    try {
      await invoke("ibkr_disconnect")
      setConnectionStatus({
        connected: false,
        server_time: null,
        client_id: connectionSettings.client_id,
      })
      setAccounts([])
      setAccountSummary([])
      setPositions([])
    } catch (err) {
      console.error("Failed to disconnect:", err)
      setError(err as string)
    }
  }

  const formatCurrency = (value: number) => {
    return new Intl.NumberFormat("en-US", {
      style: "currency",
      currency: "USD",
    }).format(value)
  }

  const formatPercent = (value: number) => {
    return `${value >= 0 ? "+" : ""}${value.toFixed(2)}%`
  }

  // Calculate account values from summary
  const getAccountValue = (tag: string): number => {
    const item = accountSummary.find((s) => s.tag === tag)
    return item ? parseFloat(item.value) : 0
  }

  const totalEquity = getAccountValue("NetLiquidation")
  const availableFunds = getAccountValue("AvailableFunds")
  const buyingPower = getAccountValue("BuyingPower")
  const dayPnL = positions.reduce((sum, pos) => sum + pos.unrealized_pnl, 0)
  const unrealizedPnL = positions.reduce((sum, pos) => sum + pos.unrealized_pnl, 0)
  const realizedPnL = positions.reduce((sum, pos) => sum + pos.realized_pnl, 0)

  return (
    <div className="min-h-screen bg-gradient-to-br from-slate-900 via-slate-800 to-slate-900 text-white p-4 space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-4xl font-bold bg-gradient-to-r from-blue-400 to-purple-400 bg-clip-text text-transparent">
            IBKR Portfolio Dashboard
          </h1>
          <p className="text-slate-400 mt-1">Interactive Brokers API Integration</p>
        </div>
        <div className="flex items-center gap-4">
          <div className="flex items-center gap-2 px-3 py-2 rounded-lg bg-slate-800/50 border border-slate-700">
            {connectionStatus.connected ? (
              <>
                <div className="w-2 h-2 bg-green-400 rounded-full animate-pulse"></div>
                <CheckCircle className="h-4 w-4 text-green-400" />
                <span className="text-sm text-green-400 font-medium">Connected</span>
              </>
            ) : (
              <>
                <div className="w-2 h-2 bg-red-400 rounded-full"></div>
                <AlertCircle className="h-4 w-4 text-red-400" />
                <span className="text-sm text-red-400 font-medium">Disconnected</span>
              </>
            )}
          </div>
          {connectionStatus.connected ? (
            <Button
              onClick={disconnectFromIBKR}
              variant="outline"
              className="border-slate-600 hover:bg-slate-800 bg-transparent"
            >
              <WifiOff className="h-4 w-4 mr-2" />
              Disconnect
            </Button>
          ) : (
            <Button
              onClick={connectToIBKR}
              disabled={loading}
              className="bg-gradient-to-r from-blue-600 to-purple-600 hover:from-blue-700 hover:to-purple-700"
            >
              <Wifi className="h-4 w-4 mr-2" />
              {loading ? "Connecting..." : "Connect"}
            </Button>
          )}
        </div>
      </div>

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
        <Card className="bg-slate-800/50 border-slate-700 backdrop-blur-sm">
          <CardHeader>
            <CardTitle className="flex items-center gap-2 text-white">
              <Settings className="h-5 w-5 text-blue-400" />
              Connection Settings
            </CardTitle>
            <CardDescription className="text-slate-400">Configure your IBKR API connection settings</CardDescription>
          </CardHeader>
          <CardContent className="grid grid-cols-1 md:grid-cols-3 gap-4">
            <div className="space-y-2">
              <Label htmlFor="host" className="text-slate-300">
                Host
              </Label>
              <Input
                id="host"
                value={connectionSettings.host}
                onChange={(e) => setConnectionSettings((prev) => ({ ...prev, host: e.target.value }))}
                placeholder="127.0.0.1"
                className="bg-slate-900/50 border-slate-600 text-white placeholder:text-slate-500"
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="port" className="text-slate-300">
                Port
              </Label>
              <Input
                id="port"
                type="number"
                value={connectionSettings.port}
                onChange={(e) => setConnectionSettings((prev) => ({ ...prev, port: parseInt(e.target.value) || 4002 }))}
                placeholder="4002"
                className="bg-slate-900/50 border-slate-600 text-white placeholder:text-slate-500"
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="clientId" className="text-slate-300">
                Client ID
              </Label>
              <Input
                id="clientId"
                type="number"
                value={connectionSettings.client_id}
                onChange={(e) => setConnectionSettings((prev) => ({ ...prev, client_id: parseInt(e.target.value) || 100 }))}
                placeholder="100"
                className="bg-slate-900/50 border-slate-600 text-white placeholder:text-slate-500"
              />
            </div>
          </CardContent>
        </Card>
      )}

      {/* Account Summary */}
      {connectionStatus.connected && (
        <>
          <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4">
            <Card className="bg-gradient-to-br from-slate-800/80 to-slate-900/80 border-slate-700 backdrop-blur-sm hover:from-slate-800/90 hover:to-slate-900/90 transition-all duration-300">
              <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
                <CardTitle className="text-sm font-medium text-slate-300">Total Equity</CardTitle>
                <div className="p-2 bg-blue-500/20 rounded-lg">
                  <DollarSign className="h-4 w-4 text-blue-400" />
                </div>
              </CardHeader>
              <CardContent>
                <div className="text-2xl font-bold text-white">{formatCurrency(totalEquity)}</div>
                <p className="text-xs text-slate-400 mt-1">
                  {accounts.length > 0 ? `Account: ${accounts[0]}` : "No account"}
                </p>
              </CardContent>
            </Card>

            <Card className="bg-gradient-to-br from-slate-800/80 to-slate-900/80 border-slate-700 backdrop-blur-sm hover:from-slate-800/90 hover:to-slate-900/90 transition-all duration-300">
              <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
                <CardTitle className="text-sm font-medium text-slate-300">Available Funds</CardTitle>
                <div className="p-2 bg-purple-500/20 rounded-lg">
                  <Activity className="h-4 w-4 text-purple-400" />
                </div>
              </CardHeader>
              <CardContent>
                <div className="text-2xl font-bold text-white">{formatCurrency(availableFunds)}</div>
                <p className="text-xs text-slate-400 mt-1">Buying Power: {formatCurrency(buyingPower)}</p>
              </CardContent>
            </Card>

            <Card className="bg-gradient-to-br from-slate-800/80 to-slate-900/80 border-slate-700 backdrop-blur-sm hover:from-slate-800/90 hover:to-slate-900/90 transition-all duration-300">
              <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
                <CardTitle className="text-sm font-medium text-slate-300">Day P&L</CardTitle>
                <div className={`p-2 rounded-lg ${dayPnL >= 0 ? "bg-green-500/20" : "bg-red-500/20"}`}>
                  {dayPnL >= 0 ? (
                    <TrendingUp className="h-4 w-4 text-green-400" />
                  ) : (
                    <TrendingDown className="h-4 w-4 text-red-400" />
                  )}
                </div>
              </CardHeader>
              <CardContent>
                <div className={`text-2xl font-bold ${dayPnL >= 0 ? "text-green-400" : "text-red-400"}`}>
                  {formatCurrency(dayPnL)}
                </div>
                <p className="text-xs text-slate-400 mt-1">Today's performance</p>
              </CardContent>
            </Card>

            <Card className="bg-gradient-to-br from-slate-800/80 to-slate-900/80 border-slate-700 backdrop-blur-sm hover:from-slate-800/90 hover:to-slate-900/90 transition-all duration-300">
              <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
                <CardTitle className="text-sm font-medium text-slate-300">Unrealized P&L</CardTitle>
                <div className="p-2 bg-orange-500/20 rounded-lg">
                  <PieChart className="h-4 w-4 text-orange-400" />
                </div>
              </CardHeader>
              <CardContent>
                <div
                  className={`text-2xl font-bold ${unrealizedPnL >= 0 ? "text-green-400" : "text-red-400"}`}
                >
                  {formatCurrency(unrealizedPnL)}
                </div>
                <p className="text-xs text-slate-400 mt-1">Open positions</p>
              </CardContent>
            </Card>
          </div>

          {/* Main Content Tabs */}
          <Tabs defaultValue="positions" className="space-y-4">
            <TabsList className="bg-slate-800/50 border border-slate-700">
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

            <TabsContent value="positions" className="space-y-4">
              <Card className="bg-slate-800/50 border-slate-700 backdrop-blur-sm">
                <CardHeader>
                  <CardTitle className="text-white flex items-center gap-2">
                    <BarChart3 className="h-5 w-5 text-blue-400" />
                    Portfolio Positions
                  </CardTitle>
                  <CardDescription className="text-slate-400">Current holdings and performance</CardDescription>
                </CardHeader>
                <CardContent>
                  {positions.length === 0 ? (
                    <p className="text-slate-400 text-center py-8">No positions found</p>
                  ) : (
                    <div className="space-y-4">
                      {positions.map((position, index) => (
                        <div
                          key={`${position.symbol}-${index}`}
                          className="flex items-center justify-between p-4 bg-slate-900/50 rounded-lg border border-slate-700 hover:bg-slate-900/70 transition-colors"
                        >
                          <div className="flex items-center gap-4">
                            <div className="w-12 h-12 bg-gradient-to-br from-blue-500 to-purple-500 rounded-lg flex items-center justify-center">
                              <span className="text-white font-bold text-sm">
                                {position.symbol.slice(0, 2).toUpperCase()}
                              </span>
                            </div>
                            <div>
                              <h3 className="font-semibold text-white">{position.symbol}</h3>
                              <p className="text-sm text-slate-400">
                                {position.position > 0 ? "LONG" : "SHORT"} {Math.abs(position.position)} shares
                              </p>
                            </div>
                          </div>
                          <div className="text-right">
                            <div className="text-lg font-semibold text-white">{formatCurrency(position.market_value)}</div>
                            <div
                              className={`text-sm flex items-center gap-1 justify-end ${position.unrealized_pnl >= 0 ? "text-green-400" : "text-red-400"}`}
                            >
                              {position.unrealized_pnl >= 0 ? (
                                <TrendingUp className="h-3 w-3" />
                              ) : (
                                <TrendingDown className="h-3 w-3" />
                              )}
                              {formatCurrency(position.unrealized_pnl)}
                            </div>
                          </div>
                        </div>
                      ))}
                    </div>
                  )}
                </CardContent>
              </Card>
            </TabsContent>

            <TabsContent value="account" className="space-y-4">
              <Card className="bg-slate-800/50 border-slate-700 backdrop-blur-sm">
                <CardHeader>
                  <CardTitle className="text-white flex items-center gap-2">
                    <Settings className="h-5 w-5 text-orange-400" />
                    Account Details
                  </CardTitle>
                  <CardDescription className="text-slate-400">Detailed account information</CardDescription>
                </CardHeader>
                <CardContent>
                  <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
                    <div className="space-y-4">
                      <div className="p-4 bg-slate-900/50 rounded-lg border border-slate-700">
                        <h4 className="text-sm font-medium text-slate-300 mb-2">Total Cash Value</h4>
                        <p className="text-2xl font-bold text-white">
                          {formatCurrency(getAccountValue("TotalCashValue"))}
                        </p>
                      </div>
                      <div className="p-4 bg-slate-900/50 rounded-lg border border-slate-700">
                        <h4 className="text-sm font-medium text-slate-300 mb-2">Realized P&L</h4>
                        <p
                          className={`text-2xl font-bold ${realizedPnL >= 0 ? "text-green-400" : "text-red-400"}`}
                        >
                          {formatCurrency(realizedPnL)}
                        </p>
                      </div>
                    </div>
                    <div className="space-y-4">
                      <div className="p-4 bg-slate-900/50 rounded-lg border border-slate-700">
                        <h4 className="text-sm font-medium text-slate-300 mb-2">Account ID</h4>
                        <p className="text-lg font-mono text-white">{accounts[0] || "N/A"}</p>
                      </div>
                      <div className="p-4 bg-slate-900/50 rounded-lg border border-slate-700">
                        <h4 className="text-sm font-medium text-slate-300 mb-2">Server Time</h4>
                        <p className="text-sm text-slate-400 flex items-center gap-2">
                          <Clock className="h-4 w-4" />
                          {connectionStatus.server_time || "N/A"}
                        </p>
                      </div>
                    </div>
                  </div>
                  
                  {accountSummary.length > 0 && (
                    <div className="mt-6">
                      <h4 className="text-sm font-medium text-slate-300 mb-4">All Account Values</h4>
                      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-3">
                        {accountSummary.map((item, index) => (
                          <div
                            key={`${item.tag}-${index}`}
                            className="p-3 bg-slate-900/50 rounded-lg border border-slate-700"
                          >
                            <p className="text-xs text-slate-400">{item.tag}</p>
                            <p className="text-sm font-medium text-white">
                              {item.value} {item.currency}
                            </p>
                          </div>
                        ))}
                      </div>
                    </div>
                  )}
                </CardContent>
              </Card>
            </TabsContent>
          </Tabs>
        </>
      )}
    </div>
  )
}