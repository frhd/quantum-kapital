import { History, Loader2, RefreshCw } from "lucide-react"
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "../../../../shared/components/ui/card"
import { Button } from "../../../../shared/components/ui/button"
import { useTickerHistory, type TickerHistorySummary } from "../../hooks/useTickerHistory"
import { useWorkspace } from "../../context/WorkspaceContext"
import { EmptyState } from "../EmptyState"
import type { PredictionWithOutcome } from "../../../eval/types"

const ROW_DISPLAY_CAP = 50

/**
 * Workspace Phase 3 — History panel. Surfaces the per-symbol
 * predictions + outcomes the eval harness records, with an accuracy
 * headline scoped to the current window. Composes
 * `eval.predictionHistory(symbol)` via `useTickerHistory` so the
 * global eval dashboard's call sites stay untouched.
 *
 * Headline math distinguishes "still pending" rows (window not yet
 * closed) from resolved misses, so a freshly-tracked symbol with all
 * predictions still in flight does not look like 0% accuracy.
 */
export function HistoryPanel() {
  const { symbol } = useWorkspace()
  const { rows, summary, loading, error, windowDays, refresh } = useTickerHistory(symbol)

  if (!symbol) {
    return (
      <EmptyState
        title="No symbol selected"
        description="Search for a ticker above to load its prediction + outcome history."
      />
    )
  }

  if (error) {
    return <EmptyState title={`Failed to load history for ${symbol}`} description={error} />
  }

  if (loading && rows.length === 0) {
    return (
      <Card className="border-border bg-card/50">
        <CardContent className="text-muted-foreground flex items-center justify-center gap-2 py-10 text-xs">
          <Loader2 className="h-4 w-4 animate-spin" />
          Loading history for {symbol}…
        </CardContent>
      </Card>
    )
  }

  if (rows.length === 0) {
    return (
      <EmptyState
        title={`No predictions for ${symbol} in the last ${windowDays} days`}
        description="Predictions show up here once the morning ranker (or another LLM loop) writes one for this symbol."
      />
    )
  }

  const visibleRows = rows.slice(0, ROW_DISPLAY_CAP)
  const truncated = rows.length > visibleRows.length

  return (
    <Card className="border-border bg-card/50 backdrop-blur-xs">
      <CardHeader>
        <div className="flex items-center justify-between gap-4">
          <div className="flex items-center gap-2">
            <History className="h-4 w-4 text-blue-300" />
            <div>
              <CardTitle className="text-foreground">History</CardTitle>
              <CardDescription className="text-muted-foreground">
                Predictions + realized outcomes for {symbol} over the last {windowDays} days.
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
        <SummaryStrip summary={summary} />
        <div className="border-border overflow-x-auto rounded-md border">
          <table className="w-full text-left text-xs">
            <thead className="bg-muted/30 text-muted-foreground">
              <tr>
                <th className="px-3 py-2 font-medium">Predicted</th>
                <th className="px-3 py-2 font-medium">Conviction</th>
                <th className="px-3 py-2 font-medium">Source</th>
                <th className="px-3 py-2 font-medium">Outcome</th>
                <th className="px-3 py-2 font-medium">Resolved</th>
              </tr>
            </thead>
            <tbody>
              {visibleRows.map((row) => (
                <HistoryRow key={row.prediction.id} row={row} />
              ))}
            </tbody>
          </table>
        </div>
        {truncated && (
          <p className="text-muted-foreground text-[11px]">
            Showing the first {visibleRows.length} of {rows.length} predictions. Open the Eval tab
            for the full history.
          </p>
        )}
      </CardContent>
    </Card>
  )
}

interface SummaryStripProps {
  summary: TickerHistorySummary
}

function SummaryStrip({ summary }: SummaryStripProps) {
  const hitRate = summary.hitRate === null ? "—" : `${(summary.hitRate * 100).toFixed(1)}%`
  return (
    <div className="grid grid-cols-2 gap-2 sm:grid-cols-4">
      <Stat
        label="Hit rate"
        value={hitRate}
        hint={
          summary.scoreable === 0
            ? "no resolved predictions yet"
            : `${summary.hits} / ${summary.scoreable} scoreable`
        }
      />
      <Stat label="Total" value={String(summary.total)} hint="in window" />
      <Stat
        label="Pending"
        value={String(summary.pending)}
        hint={summary.pending > 0 ? "eval window still open" : undefined}
      />
      <Stat
        label="Skipped"
        value={String(summary.skipped + summary.unparseable)}
        hint={
          summary.unparseable > 0
            ? `${summary.skipped} skipped · ${summary.unparseable} unparseable`
            : `${summary.skipped} skipped`
        }
      />
    </div>
  )
}

interface StatProps {
  label: string
  value: string
  hint?: string
}

function Stat({ label, value, hint }: StatProps) {
  return (
    <div className="border-border bg-background/40 rounded-md border px-3 py-2">
      <p className="text-muted-foreground text-[10px] tracking-wide uppercase">{label}</p>
      <p className="text-foreground mt-1 text-sm font-semibold">{value}</p>
      {hint && <p className="text-muted-foreground mt-0.5 text-[10px]">{hint}</p>}
    </div>
  )
}

interface HistoryRowProps {
  row: PredictionWithOutcome
}

function HistoryRow({ row }: HistoryRowProps) {
  const { prediction, outcome } = row
  const predicted = formatTimestamp(prediction.predicted_at)
  const resolved = outcome ? formatTimestamp(outcome.evaluated_at) : null
  const conviction = prediction.conviction ?? "—"
  return (
    <tr className="border-border border-t">
      <td className="text-muted-foreground px-3 py-2">{predicted}</td>
      <td className="text-foreground px-3 py-2">{conviction}</td>
      <td className="text-muted-foreground px-3 py-2">{prediction.source}</td>
      <td className="px-3 py-2">{renderOutcomeChip(outcome?.outcome_class ?? null)}</td>
      <td className="text-muted-foreground px-3 py-2">{resolved ?? "pending"}</td>
    </tr>
  )
}

function renderOutcomeChip(outcomeClass: string | null) {
  if (!outcomeClass) {
    return (
      <span className="rounded-full bg-amber-500/15 px-2 py-0.5 text-[10px] text-amber-200">
        pending
      </span>
    )
  }
  return (
    <span className={"rounded-full px-2 py-0.5 text-[10px] " + outcomeChipClasses(outcomeClass)}>
      {outcomeClass.replace(/_/g, " ")}
    </span>
  )
}

function outcomeChipClasses(c: string): string {
  switch (c) {
    case "hit_target":
      return "bg-emerald-500/20 text-emerald-200"
    case "hit_entry":
      return "bg-emerald-500/10 text-emerald-200"
    case "hit_invalidation":
      return "bg-rose-500/20 text-rose-200"
    case "drifted":
    case "no_movement":
      return "bg-slate-500/20 text-slate-200"
    case "skipped":
      return "bg-muted/40 text-muted-foreground"
    case "unparseable":
    default:
      return "bg-muted/40 text-muted-foreground"
  }
}

function formatTimestamp(iso: string): string {
  if (!iso) return "—"
  const d = new Date(iso)
  if (Number.isNaN(d.getTime())) return iso
  return d.toLocaleString()
}
