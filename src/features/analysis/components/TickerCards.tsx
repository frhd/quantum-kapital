import { Card, CardContent, CardHeader, CardTitle } from "../../../shared/components/ui/card"
import { TrendingUp, TrendingDown, DollarSign, BarChart3, PieChart, Percent } from "lucide-react"
import type { TickerData } from "../types"

interface TickerCardsProps {
  ticker: TickerData
}

export function TickerCards({ ticker }: TickerCardsProps) {
  const isPositive = (ticker.change ?? 0) >= 0

  return (
    <div className="grid grid-cols-1 gap-4 md:grid-cols-2 lg:grid-cols-4">
      {/* Price Card */}
      <Card className="border-slate-700/50 bg-slate-800/30 backdrop-blur-xs">
        <CardHeader className="pb-2">
          <CardTitle className="flex items-center gap-2 text-sm font-medium text-slate-300">
            <DollarSign className="h-4 w-4 text-blue-400/60" />
            Current Price
          </CardTitle>
        </CardHeader>
        <CardContent>
          <div className="space-y-1">
            <p className="text-3xl font-bold text-white">${ticker.price?.toFixed(2) ?? "—"}</p>
            {ticker.change !== undefined && ticker.changePercent !== undefined && (
              <div
                className={`flex items-center gap-1 text-sm ${isPositive ? "text-green-400" : "text-red-400"}`}
              >
                {isPositive ? (
                  <TrendingUp className="h-4 w-4" />
                ) : (
                  <TrendingDown className="h-4 w-4" />
                )}
                <span>
                  {isPositive ? "+" : ""}
                  {ticker.change.toFixed(2)} ({isPositive ? "+" : ""}
                  {ticker.changePercent.toFixed(2)}%)
                </span>
              </div>
            )}
          </div>
        </CardContent>
      </Card>

      {/* Volume Card */}
      <Card className="border-slate-700/50 bg-slate-800/30 backdrop-blur-xs">
        <CardHeader className="pb-2">
          <CardTitle className="flex items-center gap-2 text-sm font-medium text-slate-300">
            <BarChart3 className="h-4 w-4 text-purple-400/60" />
            Volume
          </CardTitle>
        </CardHeader>
        <CardContent>
          <div className="space-y-1">
            <p className="text-3xl font-bold text-white">
              {ticker.volume !== undefined ? (ticker.volume / 1_000_000).toFixed(2) + "M" : "—"}
            </p>
            <p className="text-sm text-slate-400">Trading Volume</p>
          </div>
        </CardContent>
      </Card>

      {/* Market Cap Card */}
      <Card className="border-slate-700/50 bg-slate-800/30 backdrop-blur-xs">
        <CardHeader className="pb-2">
          <CardTitle className="flex items-center gap-2 text-sm font-medium text-slate-300">
            <PieChart className="h-4 w-4 text-emerald-400/60" />
            Market Cap
          </CardTitle>
        </CardHeader>
        <CardContent>
          <div className="space-y-1">
            <p className="text-3xl font-bold text-white">{ticker.marketCap ?? "—"}</p>
            <p className="text-sm text-slate-400">{ticker.exchange}</p>
          </div>
        </CardContent>
      </Card>

      {/* Metrics Card */}
      <Card className="border-slate-700/50 bg-slate-800/30 backdrop-blur-xs">
        <CardHeader className="pb-2">
          <CardTitle className="flex items-center gap-2 text-sm font-medium text-slate-300">
            <Percent className="h-4 w-4 text-amber-400/60" />
            Key Metrics
          </CardTitle>
        </CardHeader>
        <CardContent>
          <div className="space-y-2">
            <div className="flex items-center justify-between">
              <span className="text-sm text-slate-400">P/E Ratio</span>
              <span className="text-lg font-semibold text-white">
                {ticker.pe?.toFixed(2) ?? "—"}
              </span>
            </div>
            <div className="flex items-center justify-between">
              <span className="text-sm text-slate-400">Yield</span>
              <span className="text-lg font-semibold text-white">
                {ticker.yield !== undefined ? ticker.yield.toFixed(2) + "%" : "—"}
              </span>
            </div>
          </div>
        </CardContent>
      </Card>
    </div>
  )
}
