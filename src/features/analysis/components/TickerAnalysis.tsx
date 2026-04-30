import { useEffect } from "react"
import { Card, CardContent } from "../../../shared/components/ui/card"
import { Skeleton } from "../../../shared/components/ui/skeleton"
import { Alert, AlertDescription } from "../../../shared/components/ui/alert"
import { TickerSearch } from "./TickerSearch"
import { TickerCards } from "./TickerCards"
import { ProjectionView } from "./ProjectionView"
import { useTickerSearch } from "../hooks/useTickerSearch"
import { useProjections } from "../hooks/useProjections"
import { useQuote } from "../hooks/useQuote"
import { AlertCircle } from "lucide-react"

interface TickerAnalysisProps {
  pendingSymbol?: { symbol: string; nonce: number } | null
}

export function TickerAnalysis({ pendingSymbol }: TickerAnalysisProps = {}) {
  const {
    searchQuery,
    searchResults,
    selectedTicker,
    loading,
    searchTickers,
    selectTicker,
    clearSelection,
  } = useTickerSearch()

  const {
    results,
    loading: projectionsLoading,
    error: projectionsError,
  } = useProjections(selectedTicker?.symbol || null)

  const { quote, error: quoteError } = useQuote(selectedTicker?.symbol || null)

  useEffect(() => {
    if (pendingSymbol) {
      selectTicker(pendingSymbol.symbol)
    }
    // selectTicker is stable from useCallback in useTickerSearch
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pendingSymbol?.nonce])

  return (
    <div className="space-y-6">
      {/* Search Input */}
      <Card className="border-border bg-card/50 relative z-50 overflow-visible backdrop-blur-xs">
        <CardContent className="overflow-visible pt-6">
          <TickerSearch
            searchQuery={searchQuery}
            searchResults={searchResults}
            onSearch={searchTickers}
            onSelect={selectTicker}
            onClear={clearSelection}
          />
        </CardContent>
      </Card>

      {/* Loading State */}
      {loading && (
        <div className="grid grid-cols-1 gap-4 md:grid-cols-2 lg:grid-cols-4">
          {[...Array(4)].map((_, i) => (
            <Card key={i} className="border-border bg-card/50">
              <CardContent className="pt-6">
                <Skeleton className="bg-secondary h-24" />
              </CardContent>
            </Card>
          ))}
        </div>
      )}

      {/* Ticker Cards */}
      {selectedTicker && !loading && (
        <TickerCards ticker={selectedTicker} quote={quote} quoteError={quoteError} />
      )}

      {/* Forward Analysis - Projections */}
      {selectedTicker && !loading && (
        <>
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
            <>
              <ProjectionView results={results} symbol={selectedTicker.symbol} />
            </>
          ) : null}
        </>
      )}
    </div>
  )
}
