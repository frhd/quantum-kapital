import { Card, CardContent } from "../../../../shared/components/ui/card"
import { Skeleton } from "../../../../shared/components/ui/skeleton"
import { Alert, AlertDescription } from "../../../../shared/components/ui/alert"
import { AlertCircle } from "lucide-react"
import { TickerCards } from "../../../analysis/components/TickerCards"
import { ProjectionView } from "../../../analysis/components/ProjectionView"
import { SentimentWidget } from "../../../sentiment/components/SentimentWidget"
import { useProjections } from "../../../analysis/hooks/useProjections"
import { useQuote } from "../../../analysis/hooks/useQuote"
import { useWorkspace } from "../../context/WorkspaceContext"
import type { TickerData } from "../../../analysis/types"

export function OverviewPanel() {
  const { symbol } = useWorkspace()
  const {
    results,
    fundamentalData,
    loading: projectionsLoading,
    error: projectionsError,
  } = useProjections(symbol)
  const { quote, error: quoteError } = useQuote(symbol)

  if (!symbol) {
    return (
      <Card className="border-border bg-card/50">
        <CardContent className="py-12 text-center">
          <p className="text-muted-foreground text-sm">
            Search for a ticker above to load fundamentals, projection, and sentiment.
          </p>
        </CardContent>
      </Card>
    )
  }

  const ticker: TickerData = fundamentalData
    ? {
        symbol: fundamentalData.symbol,
        name: fundamentalData.currentMetrics.name || symbol,
        exchange: fundamentalData.currentMetrics.exchange || "Unknown",
        type: "Stock",
        marketCap: fundamentalData.currentMetrics.marketCap || undefined,
        pe: fundamentalData.currentMetrics.peRatio,
        yield: fundamentalData.currentMetrics.dividendYield,
      }
    : { symbol, name: symbol, exchange: "Unknown", type: "Stock" }

  return (
    <div className="space-y-6">
      <TickerCards ticker={ticker} quote={quote} quoteError={quoteError} />
      <SentimentWidget symbol={symbol} />
      {projectionsLoading ? (
        <Card className="border-border bg-card/50">
          <CardContent className="pt-6">
            <Skeleton className="bg-secondary h-96" />
          </CardContent>
        </Card>
      ) : projectionsError ? (
        <Alert className="border-red-900/50 bg-red-900/20">
          <AlertCircle className="h-4 w-4 text-red-400" />
          <AlertDescription className="text-red-300">
            Failed to load projections: {projectionsError}
          </AlertDescription>
        </Alert>
      ) : results ? (
        <ProjectionView results={results} symbol={symbol} />
      ) : null}
    </div>
  )
}
