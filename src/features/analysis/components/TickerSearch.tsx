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
  return (
    <div className="relative">
      <div className="flex gap-2">
        <div className="relative flex-1">
          <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-slate-400" />
          <Input
            type="text"
            placeholder="Search ticker symbol or company name..."
            value={searchQuery}
            onChange={(e) => onSearch(e.target.value)}
            className="pl-10 bg-slate-800/50 border-slate-700 text-white placeholder:text-slate-400"
          />
        </div>
        {searchQuery && (
          <Button
            variant="outline"
            size="icon"
            onClick={onClear}
            className="bg-slate-800/50 border-slate-700 hover:bg-slate-700"
          >
            <X className="h-4 w-4" />
          </Button>
        )}
      </div>

      {/* Search Results Dropdown */}
      {searchResults.length > 0 && (
        <Card className="absolute z-50 w-full mt-2 bg-slate-800 border-slate-700 max-h-96 overflow-y-auto">
          <CardContent className="p-0">
            {searchResults.map((result) => (
              <button
                key={result.symbol}
                onClick={() => onSelect(result.symbol)}
                className="w-full text-left px-4 py-3 hover:bg-slate-700 transition-colors border-b border-slate-700 last:border-b-0"
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
