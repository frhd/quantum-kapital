import { ExternalLink, Loader2, Newspaper, RefreshCw } from "lucide-react"
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "../../../../shared/components/ui/card"
import { Button } from "../../../../shared/components/ui/button"
import { useTickerNews } from "../../hooks/useTickerNews"
import { useWorkspace } from "../../context/WorkspaceContext"
import { EmptyState } from "../EmptyState"
import type { NewsItem } from "../../../tracker/types"
import type { NewsVerdict } from "../../types"

const NEWS_DISPLAY_CAP = 50

/**
 * Workspace Phase 3 — News panel. Reads the `news_cache` SQLite row
 * for the active symbol via the cache-only `news_get_cached` Tauri
 * command. Producer-agnostic: the panel never calls a vendor adapter
 * directly, so it survives the AV→IBKR provider migration tracked in
 * `loop/plan/`.
 *
 * The panel surfaces:
 *   • A `fetched_at_unix` staleness chip so the user can judge how
 *     recent the rows are (cache rows can lag the live tape by hours).
 *   • An optional `news_verdict_json` chip showing the LLM tone /
 *     EP-worthy / parabolic-risk flags when the interpreter has run.
 *   • A capped, scrollable list of `NewsItem` rows with title, source,
 *     timestamp, summary, and an `Open` link to the source article.
 */
export function NewsPanel() {
  const { symbol } = useWorkspace()
  const { data, loading, error, verdict, refresh } = useTickerNews(symbol)

  if (!symbol) {
    return (
      <EmptyState
        title="No symbol selected"
        description="Search for a ticker above to load its cached news."
      />
    )
  }

  if (error) {
    return <EmptyState title={`Failed to load news for ${symbol}`} description={error} />
  }

  // Show the spinner whenever we have no payload yet — covers both the
  // first-fetch-in-flight case and the `data === null` race window
  // between a `setSymbol` call and the next refresh effect firing.
  if (data === null) {
    return (
      <Card className="border-border bg-card/50">
        <CardContent className="text-muted-foreground flex items-center justify-center gap-2 py-10 text-xs">
          <Loader2 className="h-4 w-4 animate-spin" />
          Loading news for {symbol}…
        </CardContent>
      </Card>
    )
  }

  const view = data
  const items = view.items.slice(0, NEWS_DISPLAY_CAP)
  const hasRow = view.fetched_at_unix > 0
  const truncated = view.items.length > items.length

  if (!hasRow) {
    return (
      <EmptyState
        title={`No cached news for ${symbol}`}
        description="The producer has not yet written a news_cache row for this symbol — try again after the next sweep."
      />
    )
  }

  if (items.length === 0) {
    return (
      <EmptyState
        title={`No news items in cache for ${symbol}`}
        description={`Last refresh: ${formatStaleness(view.fetched_at_unix)}.`}
      />
    )
  }

  return (
    <Card className="border-border bg-card/50 backdrop-blur-xs">
      <CardHeader>
        <div className="flex items-center justify-between gap-4">
          <div className="flex items-center gap-2">
            <Newspaper className="h-4 w-4 text-blue-300" />
            <div>
              <CardTitle className="text-foreground">News</CardTitle>
              <CardDescription className="text-muted-foreground">
                Cached headlines for {symbol} · {formatStaleness(view.fetched_at_unix)}
              </CardDescription>
            </div>
          </div>
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
        </div>
      </CardHeader>
      <CardContent className="space-y-3">
        <VerdictChip verdict={verdict} rawJson={view.verdict_json} />
        <div className="flex max-h-[28rem] flex-col gap-2 overflow-y-auto">
          {items.map((item, idx) => (
            <NewsRow key={`${item.url}-${idx}`} item={item} />
          ))}
        </div>
        {truncated && (
          <p className="text-muted-foreground text-[11px]">
            Showing the first {items.length} of {view.items.length} cached items.
          </p>
        )}
      </CardContent>
    </Card>
  )
}

interface NewsRowProps {
  item: NewsItem
}

function NewsRow({ item }: NewsRowProps) {
  const ts = formatTimestamp(item.time_published)
  const sentiment = item.overall_sentiment_label
  return (
    <div className="border-border bg-background/40 rounded-md border p-3 text-xs">
      <div className="text-muted-foreground flex flex-wrap items-center gap-2 text-[11px]">
        <span className="text-foreground font-medium">{item.source}</span>
        {ts && <span>{ts}</span>}
        {sentiment && (
          <span className="bg-muted/60 text-foreground/80 rounded-full px-2 py-0.5 text-[10px]">
            {sentiment}
          </span>
        )}
      </div>
      <p className="text-foreground mt-1 text-sm font-medium">{item.title}</p>
      {item.summary && (
        <p className="text-muted-foreground mt-1 line-clamp-3 text-xs">{item.summary}</p>
      )}
      {item.url && (
        <a
          href={item.url}
          target="_blank"
          rel="noopener noreferrer"
          className="text-primary hover:text-primary/80 mt-2 inline-flex items-center gap-1 text-[11px]"
        >
          Open <ExternalLink className="h-3 w-3" />
        </a>
      )}
    </div>
  )
}

interface VerdictChipProps {
  verdict: NewsVerdict | null
  rawJson: string | null
}

function VerdictChip({ verdict, rawJson }: VerdictChipProps) {
  if (!rawJson) {
    return (
      <div className="bg-muted/30 text-muted-foreground rounded-md border border-dashed border-white/10 px-3 py-2 text-[11px]">
        Verdict pending — the news interpreter has not yet scored this payload.
      </div>
    )
  }
  if (!verdict) {
    return (
      <details className="bg-muted/30 text-muted-foreground rounded-md border border-white/10 px-3 py-2 text-[11px]">
        <summary className="cursor-pointer">Verdict (raw JSON)</summary>
        <pre className="mt-2 overflow-x-auto text-[10px] break-all whitespace-pre-wrap">
          {rawJson}
        </pre>
      </details>
    )
  }
  const tone = verdict.tone ?? "unknown"
  const flags: string[] = []
  if (verdict.ep_worthy) flags.push("EP-worthy")
  if (verdict.parabolic_risk) flags.push("parabolic risk")
  return (
    <div className="border-border bg-muted/30 rounded-md border px-3 py-2 text-[11px]">
      <div className="flex flex-wrap items-center gap-2">
        <span className={"rounded-full px-2 py-0.5 text-[10px] " + toneClasses(tone)}>{tone}</span>
        {flags.map((flag) => (
          <span
            key={flag}
            className="rounded-full bg-amber-500/20 px-2 py-0.5 text-[10px] text-amber-200"
          >
            {flag}
          </span>
        ))}
      </div>
      {verdict.summary && <p className="text-muted-foreground mt-1">{verdict.summary}</p>}
    </div>
  )
}

function toneClasses(tone: string): string {
  switch (tone.toLowerCase()) {
    case "bullish":
      return "bg-emerald-500/20 text-emerald-200"
    case "bearish":
      return "bg-rose-500/20 text-rose-200"
    case "neutral":
      return "bg-slate-500/20 text-slate-200"
    default:
      return "bg-muted/40 text-muted-foreground"
  }
}

function formatTimestamp(iso: string): string | null {
  if (!iso) return null
  const d = new Date(iso)
  if (Number.isNaN(d.getTime())) return null
  return d.toLocaleString()
}

/** Render a relative "x minutes ago" label from a Unix-second timestamp. */
function formatStaleness(unix: number): string {
  if (unix <= 0) return "never refreshed"
  const seconds = Math.max(0, Math.floor(Date.now() / 1000) - unix)
  if (seconds < 60) return `${seconds}s ago`
  const minutes = Math.floor(seconds / 60)
  if (minutes < 60) return `${minutes}m ago`
  const hours = Math.floor(minutes / 60)
  if (hours < 24) return `${hours}h ago`
  const days = Math.floor(hours / 24)
  return `${days}d ago`
}
