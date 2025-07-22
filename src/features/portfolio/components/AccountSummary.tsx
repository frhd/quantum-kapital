import { Card, CardContent, CardHeader, CardTitle } from "../../../shared/components/ui/card"
import { 
  DollarSign, 
  Activity, 
  TrendingUp, 
  TrendingDown, 
  PieChart 
} from "lucide-react"
import { formatCurrency } from "../utils"
import type { AccountSummary as AccountSummaryType, Position } from "../../../shared/types"

interface AccountSummaryProps {
  accounts: string[]
  accountSummary: AccountSummaryType[]
  positions: Position[]
}

export function AccountSummary({ accounts, accountSummary, positions }: AccountSummaryProps) {
  // Calculate account values from summary - check multiple possible tag names
  const getAccountValue = (tags: string[]): number => {
    for (const tag of tags) {
      const item = accountSummary.find((s) => s.tag === tag)
      if (item) {
        const value = parseFloat(item.value)
        console.log(`Found ${tag}: ${value}`)
        return value
      }
    }
    return 0
  }

  const totalEquity = getAccountValue(["NetLiquidation", "NetLiquidationByCurrency", "TotalNetLiquidation"])
  const availableFunds = getAccountValue(["AvailableFunds", "AvailableFunds-S", "AvailableFunds-C"])
  const buyingPower = getAccountValue(["BuyingPower", "BuyingPower-S"])
  
  // Calculate P&L from positions
  const unrealizedPnL = positions.reduce((sum, pos) => sum + pos.unrealized_pnl, 0)
  const realizedPnL = positions.reduce((sum, pos) => sum + pos.realized_pnl, 0)
  const totalPnL = unrealizedPnL + realizedPnL

  return (
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
          <CardTitle className="text-sm font-medium text-slate-300">Total P&L</CardTitle>
          <div className={`p-2 rounded-lg ${totalPnL >= 0 ? "bg-green-500/20" : "bg-red-500/20"}`}>
            {totalPnL >= 0 ? (
              <TrendingUp className="h-4 w-4 text-green-400" />
            ) : (
              <TrendingDown className="h-4 w-4 text-red-400" />
            )}
          </div>
        </CardHeader>
        <CardContent>
          <div className={`text-2xl font-bold ${totalPnL >= 0 ? "text-green-400" : "text-red-400"}`}>
            {formatCurrency(totalPnL)}
          </div>
          <p className="text-xs text-slate-400 mt-1">Unrealized + Realized</p>
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
  )
}