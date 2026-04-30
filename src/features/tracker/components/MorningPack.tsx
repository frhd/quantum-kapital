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
    <Card className="border-amber-500/30 bg-amber-500/5">
      <CardHeader>
        <div className="flex items-center justify-between gap-4">
          <div className="flex items-center gap-2">
            <Sparkles className="h-5 w-5 text-amber-300" />
            <div>
              <CardTitle className="text-foreground">Morning Pack</CardTitle>
              <CardDescription className="text-muted-foreground">
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
            <p className="text-muted-foreground text-sm">
              No setups detected for this trading day.
            </p>
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
                  className="border-border bg-background/40 rounded border px-3 py-2"
                >
                  <button
                    type="button"
                    onClick={() => toggleRow(row.setup_id)}
                    className="flex w-full items-center justify-between gap-3 text-left"
                  >
                    <div className="flex min-w-0 items-center gap-3">
                      <Badge
                        variant="outline"
                        className="border-amber-400 bg-amber-500/20 text-amber-100"
                      >
                        #{row.rank}
                      </Badge>
                      <span className="text-foreground font-mono text-sm font-semibold">
                        {symbol ?? `setup#${row.setup_id}`}
                      </span>
                      {strategy && (
                        <Badge variant="outline" className="border-border text-foreground">
                          {STRATEGY_LABELS[strategy] ?? strategy}
                        </Badge>
                      )}
                      {direction && (
                        <span className="text-muted-foreground text-[10px] tracking-wide uppercase">
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
                          className="text-foreground hover:text-foreground h-7 px-2"
                          title="Open analysis"
                        >
                          <ExternalLink className="h-3.5 w-3.5" />
                        </Button>
                      )}
                      {expanded ? (
                        <ChevronDown className="text-muted-foreground h-4 w-4" />
                      ) : (
                        <ChevronRight className="text-muted-foreground h-4 w-4" />
                      )}
                    </div>
                  </button>
                  {expanded && (
                    <p className="text-foreground mt-2 text-sm leading-relaxed">
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
