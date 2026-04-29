import { Card, CardContent, CardHeader, CardTitle } from "../../../shared/components/ui/card"
import { DollarSign, Activity, TrendingUp, TrendingDown, PieChart } from "lucide-react"
import { formatCurrency, anonymizeAccountNumber } from "../utils"
import { useDailyPnL } from "../hooks/useDailyPnL"
import type { AccountSummary as AccountSummaryType, Position } from "../../../shared/types"

interface AccountSummaryProps {
  accounts: string[]
  accountSummary: AccountSummaryType[]
  positions: Position[]
}

export function AccountSummary({ accounts, accountSummary, positions }: AccountSummaryProps) {
  const dailyPnL = useDailyPnL(accounts[0])

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

  const totalEquity = getAccountValue([
    "NetLiquidation",
    "NetLiquidationByCurrency",
    "TotalNetLiquidation",
  ])
  const availableFunds = getAccountValue(["AvailableFunds", "AvailableFunds-S", "AvailableFunds-C"])
  const buyingPower = getAccountValue(["BuyingPower", "BuyingPower-S"])

  // Unrealized P&L from positions
  const unrealizedPnL = positions.reduce((sum, pos) => sum + pos.unrealized_pnl, 0)

  const dailyValue = dailyPnL?.daily_pnl ?? null

  return (
    <div className="grid grid-cols-1 gap-4 md:grid-cols-2 lg:grid-cols-4">
      <Card className="border-slate-700/50 bg-slate-800/30 backdrop-blur-xs">
        <CardHeader className="pb-2">
          <CardTitle className="flex items-center gap-2 text-sm font-medium text-slate-300">
            <DollarSign className="h-4 w-4 text-blue-400/60" />
            Total Equity
          </CardTitle>
        </CardHeader>
        <CardContent>
          <div className="space-y-1">
            <p className="text-3xl font-bold text-white">{formatCurrency(totalEquity)}</p>
            <p className="text-sm text-slate-400">
              {accounts.length > 0
                ? `Account: ${anonymizeAccountNumber(accounts[0])}`
                : "No account"}
            </p>
          </div>
        </CardContent>
      </Card>

      <Card className="border-slate-700/50 bg-slate-800/30 backdrop-blur-xs">
        <CardHeader className="pb-2">
          <CardTitle className="flex items-center gap-2 text-sm font-medium text-slate-300">
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

      <Card className="border-slate-700/50 bg-slate-800/30 backdrop-blur-xs">
        <CardHeader className="pb-2">
          <CardTitle className="flex items-center gap-2 text-sm font-medium text-slate-300">
            {(dailyValue ?? 0) >= 0 ? (
              <TrendingUp className="h-4 w-4 text-green-400/60" />
            ) : (
              <TrendingDown className="h-4 w-4 text-red-400/60" />
            )}
            Daily P&L
          </CardTitle>
        </CardHeader>
        <CardContent>
          <div className="space-y-1">
            {dailyValue === null ? (
              <p className="text-3xl font-bold text-slate-500">—</p>
            ) : (
              <p
                className={`text-3xl font-bold ${dailyValue >= 0 ? "text-green-400" : "text-red-400"}`}
              >
                {formatCurrency(dailyValue)}
              </p>
            )}
            <p className="text-sm text-slate-400">
              {dailyValue === null ? "Awaiting first tick" : "Today's change"}
            </p>
          </div>
        </CardContent>
      </Card>

      <Card className="border-slate-700/50 bg-slate-800/30 backdrop-blur-xs">
        <CardHeader className="pb-2">
          <CardTitle className="flex items-center gap-2 text-sm font-medium text-slate-300">
            <PieChart className="h-4 w-4 text-orange-400/60" />
            Unrealized P&L
          </CardTitle>
        </CardHeader>
        <CardContent>
          <div className="space-y-1">
            <p
              className={`text-3xl font-bold ${unrealizedPnL >= 0 ? "text-green-400" : "text-red-400"}`}
            >
              {formatCurrency(unrealizedPnL)}
            </p>
            <p className="text-sm text-slate-400">Open positions</p>
          </div>
        </CardContent>
      </Card>
    </div>
  )
}
