import { useEffect, useMemo, useState } from "react"
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "../../../shared/components/ui/card"
import { Button } from "../../../shared/components/ui/button"
import { Plus, RefreshCw } from "lucide-react"
import { Watchlist } from "./Watchlist"
import { useWatchlist } from "../hooks/useWatchlist"
import { STATUS_LABELS, type TrackerStatus } from "../types"

type StatusFilter = "all" | TrackerStatus

interface TrackerTabProps {
  refreshKey: number
  onSelectSymbol: (symbol: string) => void
  onAddClick: () => void
  onCountChange?: (count: number) => void
}

const FILTER_OPTIONS: ReadonlyArray<{ value: StatusFilter; label: string }> = [
  { value: "all", label: "All" },
  { value: "watching", label: STATUS_LABELS.watching },
  { value: "in_play", label: STATUS_LABELS.in_play },
  { value: "setup_active", label: STATUS_LABELS.setup_active },
  { value: "cool_down", label: STATUS_LABELS.cool_down },
]

export function TrackerTab({
  refreshKey,
  onSelectSymbol,
  onAddClick,
  onCountChange,
}: TrackerTabProps) {
  const { tickers, loading, error, refresh, remove, setTags } = useWatchlist(refreshKey)
  const [filter, setFilter] = useState<StatusFilter>("all")

  useEffect(() => {
    onCountChange?.(tickers.length)
  }, [tickers.length, onCountChange])

  const filteredTickers = useMemo(() => {
    if (filter === "all") return tickers
    return tickers.filter((t) => t.status === filter)
  }, [tickers, filter])

  return (
    <Card className="border-slate-700 bg-slate-800/50 backdrop-blur-xs">
      <CardHeader>
        <div className="flex items-center justify-between gap-4">
          <div>
            <CardTitle className="text-white">Tracker</CardTitle>
            <CardDescription className="text-slate-400">
              Watchlist of tickers being evaluated against strategy detectors.
            </CardDescription>
          </div>
          <div className="flex items-center gap-2">
            <Button
              variant="outline"
              size="sm"
              onClick={() => void refresh()}
              disabled={loading}
              className="h-8"
              title="Refresh"
            >
              <RefreshCw className={"h-4 w-4 " + (loading ? "animate-spin" : "")} />
            </Button>
            <Button size="sm" onClick={onAddClick} className="h-8">
              <Plus className="h-4 w-4" />
              Add
            </Button>
          </div>
        </div>
      </CardHeader>
      <CardContent className="space-y-3">
        <div className="flex flex-wrap items-center gap-2">
          <span className="text-xs text-slate-400">Filter:</span>
          {FILTER_OPTIONS.map((opt) => {
            const active = filter === opt.value
            const count =
              opt.value === "all"
                ? tickers.length
                : tickers.filter((t) => t.status === opt.value).length
            return (
              <button
                key={opt.value}
                type="button"
                onClick={() => setFilter(opt.value)}
                className={
                  "rounded-full border px-3 py-0.5 text-xs transition-colors " +
                  (active
                    ? "border-blue-400 bg-blue-500/20 text-blue-100"
                    : "border-slate-600 bg-slate-800 text-slate-300 hover:bg-slate-700")
                }
              >
                {opt.label} <span className="text-slate-500">({count})</span>
              </button>
            )
          })}
        </div>
        <Watchlist
          tickers={filteredTickers}
          loading={loading}
          error={error}
          onSelectSymbol={onSelectSymbol}
          onRemove={remove}
          onSaveTags={setTags}
        />
      </CardContent>
    </Card>
  )
}
