/**
 * Phase 3 — one-row, three-source social-sentiment widget.
 *
 * Renders a compact summary for the currently-selected analysis ticker:
 * Apewisdom, Stocktwits, and Reddit (WSB) latest values. Stale rows
 * are dimmed but kept visible so the user can tell "we tried, no
 * signal" from "no row at all."
 */

import { useMemo } from "react"
import { Card, CardContent, CardHeader, CardTitle } from "../../../shared/components/ui/card"
import { Badge } from "../../../shared/components/ui/badge"
import { Button } from "../../../shared/components/ui/button"
import { Skeleton } from "../../../shared/components/ui/skeleton"
import { useSocialSentiment } from "../hooks/useSocialSentiment"
import type { SocialSentimentRow } from "../types"

const SOURCE_ORDER = ["apewisdom", "stocktwits", "reddit_wsb"] as const

const SOURCE_LABEL: Record<string, string> = {
  apewisdom: "Apewisdom",
  stocktwits: "Stocktwits",
  reddit_wsb: "Reddit r/WSB",
}

interface SentimentWidgetProps {
  symbol: string | null
}

export function SentimentWidget({ symbol }: SentimentWidgetProps) {
  const { rows, loading, error, refresh, refreshing } = useSocialSentiment(symbol)

  const bySource = useMemo(() => {
    const out: Record<string, SocialSentimentRow | undefined> = {}
    for (const row of rows) out[row.source] = row
    return out
  }, [rows])

  const newestFetchedAt = useMemo(() => {
    if (!rows.length) return null
    return rows.reduce<number>((max, r) => Math.max(max, r.fetched_at), 0)
  }, [rows])

  if (!symbol) return null

  return (
    <Card className="border-border bg-card/50">
      <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
        <CardTitle className="text-sm font-medium">Social sentiment</CardTitle>
        <div className="flex items-center gap-2">
          {newestFetchedAt && (
            <span className="text-muted-foreground text-xs">{formatRelative(newestFetchedAt)}</span>
          )}
          <Button
            size="sm"
            variant="ghost"
            onClick={refresh}
            disabled={refreshing || loading}
            className="h-7 px-2 text-xs"
          >
            {refreshing ? "Refreshing…" : "Refresh"}
          </Button>
        </div>
      </CardHeader>
      <CardContent>
        {error ? (
          <p className="text-destructive text-sm">Failed to load sentiment: {error}</p>
        ) : loading && !rows.length ? (
          <div className="grid grid-cols-3 gap-3">
            {SOURCE_ORDER.map((s) => (
              <Skeleton key={s} className="bg-secondary h-16" />
            ))}
          </div>
        ) : rows.length === 0 ? (
          <p className="text-muted-foreground text-sm">
            No sentiment yet — the scheduler will populate this on its next tick.
          </p>
        ) : (
          <div className="grid grid-cols-1 gap-3 md:grid-cols-3">
            {SOURCE_ORDER.map((source) => (
              <SourceCell key={source} source={source} row={bySource[source]} />
            ))}
          </div>
        )}
      </CardContent>
    </Card>
  )
}

function SourceCell({ source, row }: { source: string; row: SocialSentimentRow | undefined }) {
  const label = SOURCE_LABEL[source] ?? source
  if (!row) {
    return (
      <div className="border-border bg-background/40 rounded-md border p-3">
        <p className="text-muted-foreground text-xs tracking-wide uppercase">{label}</p>
        <p className="text-muted-foreground mt-1 text-sm italic">no data yet</p>
      </div>
    )
  }
  const tone = row.is_stale
    ? "text-muted-foreground italic"
    : row.score === null
      ? "text-foreground"
      : row.score > 0.05
        ? "text-emerald-400"
        : row.score < -0.05
          ? "text-rose-400"
          : "text-foreground"
  return (
    <div className="border-border bg-background/40 rounded-md border p-3">
      <div className="flex items-center justify-between">
        <p className="text-muted-foreground text-xs tracking-wide uppercase">{label}</p>
        {row.sentiment_label && (
          <Badge variant="outline" className="text-xs">
            {row.sentiment_label}
          </Badge>
        )}
      </div>
      <div className={`mt-1 flex items-baseline gap-2 ${tone}`}>
        <span className="text-base font-semibold">
          {row.score === null ? "—" : row.score.toFixed(2)}
        </span>
        {row.mentions_24h !== null && (
          <span className="text-muted-foreground text-xs">
            {row.mentions_24h.toLocaleString()} mentions/24h
          </span>
        )}
      </div>
      {row.is_stale && <p className="text-muted-foreground mt-1 text-xs">no upstream signal</p>}
      {row.rank !== null && <p className="text-muted-foreground mt-1 text-xs">rank #{row.rank}</p>}
    </div>
  )
}

function formatRelative(unixSeconds: number): string {
  const ageS = Math.max(0, Math.floor(Date.now() / 1000 - unixSeconds))
  if (ageS < 60) return `${ageS}s ago`
  if (ageS < 3600) return `${Math.floor(ageS / 60)}m ago`
  if (ageS < 86_400) return `${Math.floor(ageS / 3600)}h ago`
  return `${Math.floor(ageS / 86_400)}d ago`
}
