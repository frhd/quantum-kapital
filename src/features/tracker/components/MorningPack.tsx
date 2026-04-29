import { useCallback, useEffect, useState } from "react"
import { ChevronDown, ChevronRight, ExternalLink, RefreshCw, Sparkles } from "lucide-react"
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "../../../shared/components/ui/card"
import { Badge } from "../../../shared/components/ui/badge"
import { Button } from "../../../shared/components/ui/button"
import { Skeleton } from "../../../shared/components/ui/skeleton"
import { ibkrApi } from "../../../shared/api/ibkr"
import type { MorningPack, MorningPackReadyPayload, Setup } from "../types"

interface MorningPackPanelProps {
  /** Latest `morning-pack-ready` event from `useTrackerEvents`. The
   *  panel re-fetches whenever this changes. */
  lastMorningPackReady: MorningPackReadyPayload | null
  /** Per-symbol active setup map from `useTrackerEvents` — used to
   *  light up the deep-link button when the ranked setup is still
   *  the active one for its symbol. */
  activeSetupBySymbol?: Record<string, Setup>
  onSelectSymbol?: (symbol: string) => void
}

const STRATEGY_LABELS: Record<string, string> = {
  breakout: "Breakout",
  episodic_pivot: "Episodic Pivot",
  parabolic_short: "Parabolic Short",
}

function formatPackDate(iso: string): string {
  const d = new Date(`${iso}T00:00:00`)
  if (Number.isNaN(d.getTime())) return iso
  return d.toLocaleDateString(undefined, {
    weekday: "short",
    month: "short",
    day: "numeric",
  })
}

function formatGenerated(iso: string): string {
  const d = new Date(iso)
  if (Number.isNaN(d.getTime())) return ""
  return d.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" })
}

export function MorningPackPanel({
  lastMorningPackReady,
  activeSetupBySymbol = {},
  onSelectSymbol,
}: MorningPackPanelProps) {
  const [pack, setPack] = useState<MorningPack | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [collapsed, setCollapsed] = useState(false)
  const [expandedRowId, setExpandedRowId] = useState<number | null>(null)

  const refresh = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      const result = await ibkrApi.tracker.getMorningPack()
      setPack(result)
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    void refresh()
  }, [refresh])

  // Re-fetch whenever a new MorningPackReady event arrives.
  useEffect(() => {
    if (!lastMorningPackReady) return
    void refresh()
  }, [lastMorningPackReady, refresh])

  const toggleRow = (id: number) => {
    setExpandedRowId((prev) => (prev === id ? null : id))
  }

  if (!loading && !pack && !error) {
    return null
  }

  return (
    <Card className="border-amber-500/30 bg-gradient-to-br from-amber-500/10 to-slate-800/50 backdrop-blur-xs">
      <CardHeader>
        <div className="flex items-center justify-between gap-4">
          <div className="flex items-center gap-2">
            <Sparkles className="h-5 w-5 text-amber-300" />
            <div>
              <CardTitle className="text-white">Morning Pack</CardTitle>
              <CardDescription className="text-slate-400">
                {pack
                  ? `Top ${pack.ranked.length} for ${formatPackDate(pack.date)} — generated ${formatGenerated(pack.generated_at)}`
                  : "No pack yet — runs after the EOD sweep at 16:05 ET."}
              </CardDescription>
            </div>
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
            <Button
              variant="outline"
              size="sm"
              onClick={() => setCollapsed((c) => !c)}
              className="h-8"
              title={collapsed ? "Expand" : "Collapse"}
            >
              {collapsed ? (
                <ChevronRight className="h-4 w-4" />
              ) : (
                <ChevronDown className="h-4 w-4" />
              )}
            </Button>
          </div>
        </div>
      </CardHeader>
      {!collapsed && (
        <CardContent className="space-y-2">
          {error && (
            <div className="rounded border border-rose-500/40 bg-rose-500/10 px-3 py-2 text-sm text-rose-200">
              {error}
            </div>
          )}
          {loading && !pack && (
            <div className="space-y-2">
              <Skeleton className="h-10 w-full" />
              <Skeleton className="h-10 w-full" />
              <Skeleton className="h-10 w-full" />
            </div>
          )}
          {pack && pack.ranked.length === 0 && (
            <p className="text-sm text-slate-400">No setups detected for this trading day.</p>
          )}
          {pack &&
            pack.ranked.map((row) => {
              const active = activeSetupBySymbol
                ? Object.values(activeSetupBySymbol).find((s) => s.id === row.setup_id)
                : undefined
              const symbol = active?.symbol
              const strategy = active?.strategy
              const direction = active?.direction
              const expanded = expandedRowId === row.setup_id
              return (
                <div
                  key={row.setup_id}
                  className="rounded border border-slate-700 bg-slate-900/40 px-3 py-2"
                >
                  <button
                    type="button"
                    onClick={() => toggleRow(row.setup_id)}
                    className="flex w-full items-center justify-between gap-3 text-left"
                  >
                    <div className="flex items-center gap-3 min-w-0">
                      <Badge
                        variant="outline"
                        className="border-amber-400 bg-amber-500/20 text-amber-100"
                      >
                        #{row.rank}
                      </Badge>
                      <span className="font-mono text-sm font-semibold text-white">
                        {symbol ?? `setup#${row.setup_id}`}
                      </span>
                      {strategy && (
                        <Badge variant="outline" className="border-slate-500 text-slate-200">
                          {STRATEGY_LABELS[strategy] ?? strategy}
                        </Badge>
                      )}
                      {direction && (
                        <span className="text-[10px] uppercase tracking-wide text-slate-400">
                          {direction}
                        </span>
                      )}
                    </div>
                    <div className="flex items-center gap-2">
                      {symbol && onSelectSymbol && (
                        <Button
                          variant="ghost"
                          size="sm"
                          onClick={(e) => {
                            e.stopPropagation()
                            onSelectSymbol(symbol)
                          }}
                          className="h-7 px-2 text-slate-300 hover:text-white"
                          title="Open analysis"
                        >
                          <ExternalLink className="h-3.5 w-3.5" />
                        </Button>
                      )}
                      {expanded ? (
                        <ChevronDown className="h-4 w-4 text-slate-400" />
                      ) : (
                        <ChevronRight className="h-4 w-4 text-slate-400" />
                      )}
                    </div>
                  </button>
                  {expanded && (
                    <p className="mt-2 text-sm leading-relaxed text-slate-300">
                      {row.why_top_pick}
                    </p>
                  )}
                </div>
              )
            })}
        </CardContent>
      )}
    </Card>
  )
}
