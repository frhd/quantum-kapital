import { Card, CardContent } from "../../../shared/components/ui/card"
import { Skeleton } from "../../../shared/components/ui/skeleton"
import { TickerSearch } from "./TickerSearch"
import { TickerCards } from "./TickerCards"
import { ScenarioTabs } from "./ScenarioTabs"
import { useTickerSearch } from "../hooks/useTickerSearch"
import { useProjections } from "../hooks/useProjections"

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

  const { projections, loading: projectionsLoading } = useProjections(
    selectedTicker?.symbol || null
  )

  return (
    <div className="space-y-6">
      {/* Search Input */}
      <Card className="bg-slate-800/50 border-slate-700 backdrop-blur-sm">
        <CardContent className="pt-6">
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
          ) : projections ? (
            <ScenarioTabs projections={projections} symbol={selectedTicker.symbol} />
          ) : null}
        </>
      )}

      {/* Empty State */}
      {!selectedTicker && !loading && (
        <Card className="bg-slate-800/50 border-slate-700 backdrop-blur-sm">
          <CardContent className="text-center py-12">
            <p className="text-slate-400 text-lg">
              Search for a ticker symbol to view detailed analysis
            </p>
            <p className="text-slate-500 text-sm mt-2">
              Try searching for AAPL, MSFT, GOOGL, or any other symbol
            </p>
          </CardContent>
        </Card>
      )}
    </div>
  )
}
