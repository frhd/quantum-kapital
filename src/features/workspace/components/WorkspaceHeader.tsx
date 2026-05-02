import { Card, CardContent } from "../../../shared/components/ui/card"
import { TickerSearch } from "../../analysis/components/TickerSearch"
import { useTickerSearch } from "../../analysis/hooks/useTickerSearch"
import { useWorkspace } from "../context/WorkspaceContext"
import { useTickerNavigate } from "../hooks/useTickerNavigate"

export function WorkspaceHeader() {
  const { symbol } = useWorkspace()
  const navigate = useTickerNavigate()
  const { searchQuery, searchResults, searchTickers, clearSelection } = useTickerSearch()

  const handleSelect = (next: string) => {
    clearSelection()
    navigate(next)
  }

  return (
    <Card className="border-border bg-card/50 relative z-50 overflow-visible backdrop-blur-xs">
      <CardContent className="overflow-visible pt-6">
        <TickerSearch
          searchQuery={searchQuery}
          searchResults={searchResults}
          onSearch={searchTickers}
          onSelect={handleSelect}
          onClear={clearSelection}
        />
        {symbol && (
          <p className="text-muted-foreground mt-3 text-xs">
            Active symbol: <span className="text-foreground font-mono">{symbol}</span>
          </p>
        )}
      </CardContent>
    </Card>
  )
}
