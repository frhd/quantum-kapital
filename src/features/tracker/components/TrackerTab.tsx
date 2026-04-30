import { useEffect, useMemo, useRef, useState } from "react"
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "../../../shared/components/ui/card"
import { Button } from "../../../shared/components/ui/button"
import { Plus, RefreshCw } from "lucide-react"
import { ToastViewport, useToasts } from "../../../shared/components/ui/toast"
import { Watchlist } from "./Watchlist"
import { MorningPackPanel } from "./MorningPack"
import { AlertFeed } from "./AlertFeed"
import { useWatchlist } from "../hooks/useWatchlist"
import { useTrackerEvents } from "../hooks/useTrackerEvents"
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

const STRATEGY_LABELS: Record<string, string> = {
  breakout: "Breakout",
  episodic_pivot: "Episodic Pivot",
  parabolic_short: "Parabolic Short",
}

function strategyLabel(strategy: string): string {
  return STRATEGY_LABELS[strategy] ?? strategy
}

function truncate(s: string, max: number): string {
  if (s.length <= max) return s
  return s.slice(0, max - 1).trimEnd() + "…"
}

export function TrackerTab({
  refreshKey,
  onSelectSymbol,
  onAddClick,
  onCountChange,
}: TrackerTabProps) {
  const { tickers, loading, error, refresh, remove, setTags } = useWatchlist(refreshKey)
  const {
    lastSetupDetected,
    lastInvalidated,
    lastStatusChanged,
    lastMorningPackReady,
    activeSetupBySymbol,
  } = useTrackerEvents()
  const { toasts, push: pushToast, dismiss } = useToasts()
  const [filter, setFilter] = useState<StatusFilter>("all")

  const lastSetupIdRef = useRef<string | null>(null)
  const lastInvalidatedIdRef = useRef<number | null>(null)
  const lastStatusKeyRef = useRef<string | null>(null)

  useEffect(() => {
    onCountChange?.(tickers.length)
  }, [tickers.length, onCountChange])

  // Setup detected → toast + refresh row. The same setup id may fire
  // twice (Phase 17): first with `thesis: null` from the runner, then
  // again with a populated thesis from the LLM pipeline. We re-fire
  // the toast (with refreshed copy) when the thesis arrives.
  useEffect(() => {
    if (!lastSetupDetected) return
    const setup = lastSetupDetected.setup
    const thesisMd = lastSetupDetected.thesis ?? setup.thesis ?? null
    const dedupeKey = `${setup.id}:${thesisMd ? "thesis" : "pending"}`
    if (lastSetupIdRef.current === dedupeKey) return
    lastSetupIdRef.current = dedupeKey
    const description = thesisMd
      ? truncate(thesisMd, 220)
      : `${setup.direction.toUpperCase()} @ $${setup.trigger_price.toFixed(2)} — thesis pending`
    pushToast({
      id: `setup-detected-${setup.id}-${thesisMd ? "thesis" : "pending"}`,
      variant: "success",
      title: `${strategyLabel(setup.strategy)} detected on ${setup.symbol}`,
      description,
      durationMs: thesisMd ? 9000 : 5000,
    })
    void refresh()
  }, [lastSetupDetected, pushToast, refresh])

  // Setup invalidated → toast + refresh row.
  useEffect(() => {
    if (!lastInvalidated) return
    const id = lastInvalidated.setup_id
    if (lastInvalidatedIdRef.current === id) return
    lastInvalidatedIdRef.current = id
    pushToast({
      id: `setup-invalidated-${id}`,
      variant: "warning",
      title: `${lastInvalidated.symbol} setup invalidated`,
      description: lastInvalidated.reason,
    })
    void refresh()
  }, [lastInvalidated, pushToast, refresh])

  // Status changed (not specific to a setup) → quietly refresh the
  // watchlist so the row re-renders with fresh status / TTL.
  useEffect(() => {
    if (!lastStatusChanged) return
    const key = `${lastStatusChanged.symbol}:${lastStatusChanged.from}:${lastStatusChanged.to}`
    if (lastStatusKeyRef.current === key) return
    lastStatusKeyRef.current = key
    void refresh()
  }, [lastStatusChanged, refresh])

  const filteredTickers = useMemo(() => {
    if (filter === "all") return tickers
    return tickers.filter((t) => t.status === filter)
  }, [tickers, filter])

  return (
    <>
      <div className="space-y-4">
        <MorningPackPanel
          lastMorningPackReady={lastMorningPackReady}
          activeSetupBySymbol={activeSetupBySymbol}
          onSelectSymbol={onSelectSymbol}
        />
        <Card className="border-border bg-card/50 backdrop-blur-xs">
          <CardHeader>
            <div className="flex items-center justify-between gap-4">
              <div>
                <CardTitle className="text-foreground">Tracker</CardTitle>
                <CardDescription className="text-muted-foreground">
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
              <span className="text-muted-foreground text-xs">Filter:</span>
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
                        : "border-input bg-card text-foreground hover:bg-secondary")
                    }
                  >
                    {opt.label} <span className="text-muted-foreground">({count})</span>
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
              activeSetupBySymbol={activeSetupBySymbol}
            />
          </CardContent>
        </Card>
        <AlertFeed
          lastSetupDetected={lastSetupDetected}
          lastInvalidated={lastInvalidated}
          lastStatusChanged={lastStatusChanged}
          onSelectSymbol={onSelectSymbol}
        />
      </div>
      <ToastViewport toasts={toasts} onDismiss={dismiss} />
    </>
  )
}
