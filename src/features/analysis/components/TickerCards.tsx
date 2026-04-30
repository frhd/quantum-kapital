import { Card, CardContent, CardHeader, CardTitle } from "../../../shared/components/ui/card"
import { TrendingUp, TrendingDown, DollarSign, BarChart3, PieChart, Percent } from "lucide-react"
import type { TickerData } from "../types"
import type { Quote } from "../../../shared/types"
import type { QuoteError } from "../hooks/useQuote"

interface TickerCardsProps {
  ticker: TickerData
  quote: Quote | null
  quoteError: QuoteError | null
}

function formatVolume(volume: number): string {
  return (volume / 1_000_000).toFixed(2) + "M"
}

function quoteStatusMessage(error: QuoteError | null): string | null {
  switch (error) {
    case "disconnected":
      return "Live quote unavailable — TWS not connected"
    case "no_permission":
      return "No live data permission for this symbol"
    case "timeout":
    case "fetch_failed":
      return null // em-dashes only; not worth a UI message for transient errors
    case null:
      return null
  }
}

export function TickerCards({ ticker, quote, quoteError }: TickerCardsProps) {
  const lastPrice = quote?.lastPrice
  const prevClose = quote?.prevClose
  const change =
    lastPrice !== undefined && prevClose !== undefined ? lastPrice - prevClose : undefined
  const changePercent =
    change !== undefined && prevClose !== undefined && prevClose !== 0
      ? (change / prevClose) * 100
      : undefined
  const isPositive = (change ?? 0) >= 0
  const statusMessage = quoteStatusMessage(quoteError)

  return (
    <div className="grid grid-cols-1 gap-4 md:grid-cols-2 lg:grid-cols-4">
      {/* Price Card */}
      <Card className="border-border/50 bg-card/30 backdrop-blur-xs">
        <CardHeader className="pb-2">
          <CardTitle className="text-foreground flex items-center gap-2 text-sm font-medium">
            <DollarSign className="h-4 w-4 text-blue-400/60" />
            Current Price
          </CardTitle>
        </CardHeader>
        <CardContent>
          <div className="space-y-1">
            <p className="text-foreground text-3xl font-bold">
              {lastPrice !== undefined ? `$${lastPrice.toFixed(2)}` : "—"}
            </p>
            {change !== undefined && changePercent !== undefined && (
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
                  {change.toFixed(2)} ({isPositive ? "+" : ""}
                  {changePercent.toFixed(2)}%)
                </span>
              </div>
            )}
            {statusMessage && <p className="text-muted-foreground text-xs">{statusMessage}</p>}
          </div>
        </CardContent>
      </Card>

      {/* Volume Card */}
      <Card className="border-border/50 bg-card/30 backdrop-blur-xs">
        <CardHeader className="pb-2">
          <CardTitle className="text-foreground flex items-center gap-2 text-sm font-medium">
            <BarChart3 className="h-4 w-4 text-purple-400/60" />
            Volume
          </CardTitle>
        </CardHeader>
        <CardContent>
          <div className="space-y-1">
            <p className="text-foreground text-3xl font-bold">
              {quote?.volume !== undefined ? formatVolume(quote.volume) : "—"}
            </p>
            <p className="text-muted-foreground text-sm">Trading Volume</p>
          </div>
        </CardContent>
      </Card>

      {/* Market Cap Card */}
      <Card className="border-border/50 bg-card/30 backdrop-blur-xs">
        <CardHeader className="pb-2">
          <CardTitle className="text-foreground flex items-center gap-2 text-sm font-medium">
            <PieChart className="h-4 w-4 text-emerald-400/60" />
            Market Cap
          </CardTitle>
        </CardHeader>
        <CardContent>
          <div className="space-y-1">
            <p className="text-foreground text-3xl font-bold">{ticker.marketCap ?? "—"}</p>
            <p className="text-muted-foreground text-sm">{ticker.exchange}</p>
          </div>
        </CardContent>
      </Card>

      {/* Metrics Card */}
      <Card className="border-border/50 bg-card/30 backdrop-blur-xs">
        <CardHeader className="pb-2">
          <CardTitle className="text-foreground flex items-center gap-2 text-sm font-medium">
            <Percent className="h-4 w-4 text-amber-400/60" />
            Key Metrics
          </CardTitle>
        </CardHeader>
        <CardContent>
          <div className="space-y-2">
            <div className="flex items-center justify-between">
              <span className="text-muted-foreground text-sm">P/E Ratio</span>
              <span className="text-foreground text-lg font-semibold">
                {ticker.pe?.toFixed(2) ?? "—"}
              </span>
            </div>
            <div className="flex items-center justify-between">
              <span className="text-muted-foreground text-sm">Yield</span>
              <span className="text-foreground text-lg font-semibold">
                {ticker.yield !== undefined ? ticker.yield.toFixed(2) + "%" : "—"}
              </span>
            </div>
          </div>
        </CardContent>
      </Card>
    </div>
  )
}
