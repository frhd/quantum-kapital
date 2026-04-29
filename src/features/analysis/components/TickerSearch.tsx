import { Search, X } from "lucide-react"
import { Input } from "../../../shared/components/ui/input"
import { Card, CardContent } from "../../../shared/components/ui/card"
import { Button } from "../../../shared/components/ui/button"
import type { TickerSearchResult } from "../types"

interface TickerSearchProps {
  searchQuery: string
  searchResults: TickerSearchResult[]
  onSearch: (query: string) => void
  onSelect: (symbol: string) => void
  onClear: () => void
}

export function TickerSearch({
  searchQuery,
  searchResults,
  onSearch,
  onSelect,
  onClear,
}: TickerSearchProps) {
  const handleKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Enter" && searchQuery.trim()) {
      // If there's a search result, select the first one
      // Otherwise, search for the entered symbol directly
      if (searchResults.length > 0) {
        onSelect(searchResults[0].symbol)
      } else {
        onSelect(searchQuery.trim())
      }
    }
  }

  return (
    <div className="relative">
      <div className="flex gap-2">
        <div className="relative flex-1">
          <Search className="absolute top-1/2 left-3 h-4 w-4 -translate-y-1/2 text-slate-400" />
          <Input
            type="text"
            placeholder="Search ticker symbol (e.g., AAPL) or press Enter..."
            value={searchQuery}
            onChange={(e) => onSearch(e.target.value)}
            onKeyDown={handleKeyDown}
            className="border-slate-700 bg-slate-800/50 pl-10 text-white placeholder:text-slate-400"
          />
        </div>
        {searchQuery && (
          <Button
            variant="outline"
            size="icon"
            onClick={onClear}
            className="border-slate-700 bg-slate-800/50 hover:bg-slate-700"
          >
            <X className="h-4 w-4" />
          </Button>
        )}
      </div>

      {/* Search Results Dropdown */}
      {searchResults.length > 0 && (
        <Card className="absolute z-50 mt-2 max-h-96 w-full overflow-y-auto border-slate-700 bg-slate-800">
          <CardContent className="p-0">
            {searchResults.map((result) => (
              <button
                key={result.symbol}
                onClick={() => onSelect(result.symbol)}
                className="w-full border-b border-slate-700 px-4 py-3 text-left transition-colors last:border-b-0 hover:bg-slate-700"
              >
                <div className="flex items-center justify-between">
                  <div>
                    <p className="font-semibold text-white">{result.symbol}</p>
                    <p className="text-sm text-slate-400">{result.name}</p>
                  </div>
                  <div className="text-right">
                    <p className="text-xs text-slate-500">{result.exchange}</p>
                    <p className="text-xs text-slate-500">{result.type}</p>
                  </div>
                </div>
              </button>
            ))}
          </CardContent>
        </Card>
      )}
    </div>
  )
}
