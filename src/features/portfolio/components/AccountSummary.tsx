import { Card, CardContent, CardHeader, CardTitle } from "../../../shared/components/ui/card"
import {
  DollarSign,
  Activity,
  TrendingUp,
  TrendingDown,
  PieChart
} from "lucide-react"
import { formatCurrency, anonymizeAccountNumber } from "../utils"
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
      <Card className="bg-slate-800/30 border-slate-700/50 backdrop-blur-sm">
        <CardHeader className="pb-2">
          <CardTitle className="text-sm font-medium text-slate-300 flex items-center gap-2">
            <DollarSign className="h-4 w-4 text-blue-400/60" />
            Total Equity
          </CardTitle>
        </CardHeader>
        <CardContent>
          <div className="space-y-1">
            <p className="text-3xl font-bold text-white">{formatCurrency(totalEquity)}</p>
            <p className="text-sm text-slate-400">
              {accounts.length > 0 ? `Account: ${anonymizeAccountNumber(accounts[0])}` : "No account"}
            </p>
          </div>
        </CardContent>
      </Card>

      <Card className="bg-slate-800/30 border-slate-700/50 backdrop-blur-sm">
        <CardHeader className="pb-2">
          <CardTitle className="text-sm font-medium text-slate-300 flex items-center gap-2">
            <Activity className="h-4 w-4 text-purple-400/60" />
            Available Funds
          </CardTitle>
        </CardHeader>
        <CardContent>
          <div className="space-y-1">
            <p className="text-3xl font-bold text-white">{formatCurrency(availableFunds)}</p>
            <p className="text-sm text-slate-400">Buying Power: {formatCurrency(buyingPower)}</p>
          </div>
        </CardContent>
      </Card>

      <Card className="bg-slate-800/30 border-slate-700/50 backdrop-blur-sm">
        <CardHeader className="pb-2">
          <CardTitle className="text-sm font-medium text-slate-300 flex items-center gap-2">
            {totalPnL >= 0 ? (
              <TrendingUp className="h-4 w-4 text-green-400/60" />
            ) : (
              <TrendingDown className="h-4 w-4 text-red-400/60" />
            )}
            Total P&L
          </CardTitle>
        </CardHeader>
        <CardContent>
          <div className="space-y-1">
            <p className={`text-3xl font-bold ${totalPnL >= 0 ? "text-green-400" : "text-red-400"}`}>
              {formatCurrency(totalPnL)}
            </p>
            <p className="text-sm text-slate-400">Unrealized + Realized</p>
          </div>
        </CardContent>
      </Card>

      <Card className="bg-slate-800/30 border-slate-700/50 backdrop-blur-sm">
        <CardHeader className="pb-2">
          <CardTitle className="text-sm font-medium text-slate-300 flex items-center gap-2">
            <PieChart className="h-4 w-4 text-orange-400/60" />
            Unrealized P&L
          </CardTitle>
        </CardHeader>
        <CardContent>
          <div className="space-y-1">
            <p className={`text-3xl font-bold ${unrealizedPnL >= 0 ? "text-green-400" : "text-red-400"}`}>
              {formatCurrency(unrealizedPnL)}
            </p>
            <p className="text-sm text-slate-400">Open positions</p>
          </div>
        </CardContent>
      </Card>
    </div>
  )
}
