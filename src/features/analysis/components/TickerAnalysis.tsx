import { Card, CardContent } from "../../../shared/components/ui/card"
import { Skeleton } from "../../../shared/components/ui/skeleton"
import { Alert, AlertDescription } from "../../../shared/components/ui/alert"
import { TickerSearch } from "./TickerSearch"
import { TickerCards } from "./TickerCards"
import { ProjectionView } from "./ProjectionView"
import { GoogleSheetsExport } from "./GoogleSheetsExport"
import { useTickerSearch } from "../hooks/useTickerSearch"
import { useProjections } from "../hooks/useProjections"
import { convertToTickerAnalysisData } from "../utils/exportHelpers"
import { AlertCircle } from "lucide-react"

export function TickerAnalysis() {
  const {
    searchQuery,
    searchResults,
    selectedTicker,
    loading,
    searchTickers,
    selectTicker,
    clearSelection,
  } = useTickerSearch()

  const { results, fundamentalData, loading: projectionsLoading, error: projectionsError } = useProjections(
    selectedTicker?.symbol || null
  )

  return (
    <div className="space-y-6">
      {/* Search Input */}
      <Card className="bg-slate-800/50 border-slate-700 backdrop-blur-sm overflow-visible relative z-50">
        <CardContent className="pt-6 overflow-visible">
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
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4">
          {[...Array(4)].map((_, i) => (
            <Card key={i} className="bg-slate-800/50 border-slate-700">
              <CardContent className="pt-6">
                <Skeleton className="h-24 bg-slate-700" />
              </CardContent>
            </Card>
          ))}
        </div>
      )}

      {/* Ticker Cards */}
      {selectedTicker && !loading && <TickerCards ticker={selectedTicker} />}

      {/* Forward Analysis - Projections */}
      {selectedTicker && !loading && (
        <>
          {projectionsLoading ? (
            <Card className="bg-slate-800/50 border-slate-700">
              <CardContent className="pt-6">
                <Skeleton className="h-96 bg-slate-700" />
              </CardContent>
            </Card>
          ) : projectionsError ? (
            <Alert className="bg-red-900/20 border-red-900/50">
              <AlertCircle className="h-4 w-4 text-red-400" />
              <AlertDescription className="text-red-300">
                Failed to load projections: {projectionsError}
              </AlertDescription>
            </Alert>
          ) : results ? (
            <>
              <ProjectionView results={results} symbol={selectedTicker.symbol} />
              {fundamentalData && (
                <div className="flex justify-end">
                  <GoogleSheetsExport
                    ticker={selectedTicker.symbol}
                    analysisData={convertToTickerAnalysisData(fundamentalData, results)}
                  />
                </div>
              )}
            </>
          ) : null}
        </>
      )}
    </div>
  )
}
