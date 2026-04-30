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
          <Search className="text-muted-foreground absolute top-1/2 left-3 h-4 w-4 -translate-y-1/2" />
          <Input
            type="text"
            placeholder="Search ticker symbol (e.g., AAPL) or press Enter..."
            value={searchQuery}
            onChange={(e) => onSearch(e.target.value)}
            onKeyDown={handleKeyDown}
            className="border-border bg-card/50 text-foreground placeholder:text-muted-foreground pl-10"
          />
        </div>
        {searchQuery && (
          <Button
            variant="outline"
            size="icon"
            onClick={onClear}
            className="border-border bg-card/50 hover:bg-secondary"
          >
            <X className="h-4 w-4" />
          </Button>
        )}
      </div>

      {/* Search Results Dropdown */}
      {searchResults.length > 0 && (
        <Card className="border-border bg-card absolute z-50 mt-2 max-h-96 w-full overflow-y-auto">
          <CardContent className="p-0">
            {searchResults.map((result) => (
              <button
                key={result.symbol}
                onClick={() => onSelect(result.symbol)}
                className="border-border hover:bg-secondary w-full border-b px-4 py-3 text-left transition-colors last:border-b-0"
              >
                <div className="flex items-center justify-between">
                  <div>
                    <p className="text-foreground font-semibold">{result.symbol}</p>
                    <p className="text-muted-foreground text-sm">{result.name}</p>
                  </div>
                  <div className="text-right">
                    <p className="text-muted-foreground text-xs">{result.exchange}</p>
                    <p className="text-muted-foreground text-xs">{result.type}</p>
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
