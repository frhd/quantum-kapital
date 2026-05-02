import { useMemo, useState } from "react"
import { Pencil } from "lucide-react"
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "../../../../shared/components/ui/card"
import { Button } from "../../../../shared/components/ui/button"
import { Badge } from "../../../../shared/components/ui/badge"
import { TagEditor } from "../../../tracker/components/TagEditor"
import { SetupBadge } from "../../../tracker/components/SetupBadge"
import { useWatchlist } from "../../../tracker/hooks/useWatchlist"
import { useTrackerEvents } from "../../../tracker/hooks/useTrackerEvents"
import { BUILT_IN_TAGS, STATUS_LABELS, type StrategyTag } from "../../../tracker/types"
import { useWorkspace } from "../../context/WorkspaceContext"
import { useAddToTrackerOpen } from "../../context/AddToTrackerContext"
import { EmptyState } from "../EmptyState"

function tagLabel(tag: StrategyTag): string {
  const builtin = BUILT_IN_TAGS.find((b) => b.value === tag)
  return builtin ? builtin.label : tag
}

/**
 * Workspace Phase 2 — per-symbol watchlist metadata. Renders the
 * tracker row's tags (inline editor), strategy/status badge and the
 * latest active setup pulled from `useTrackerEvents`. Untracked
 * symbols see an "Add to tracker" CTA that opens the app-level
 * dialog through `useAddToTrackerOpen()`.
 */
export function WatchlistMetaPanel() {
  const { symbol } = useWorkspace()
  const { tickers, loading, error, setTags } = useWatchlist()
  const { activeSetupBySymbol } = useTrackerEvents()
  const openAddToTracker = useAddToTrackerOpen()
  const [editing, setEditing] = useState(false)
  const [saveError, setSaveError] = useState<string | null>(null)

  const ticker = useMemo(
    () => (symbol ? (tickers.find((t) => t.symbol === symbol) ?? null) : null),
    [symbol, tickers],
  )
  const activeSetup = symbol ? activeSetupBySymbol[symbol] : undefined

  if (!symbol) {
    return (
      <EmptyState
        title="No symbol selected"
        description="Search for a ticker above to view its tracker metadata."
      />
    )
  }

  if (loading && !ticker) {
    return (
      <Card className="border-border bg-card/50">
        <CardContent className="text-muted-foreground py-10 text-center text-sm">
          Loading watchlist…
        </CardContent>
      </Card>
    )
  }

  if (error) {
    return <EmptyState title="Failed to load watchlist" description={error} />
  }

  if (!ticker) {
    return (
      <EmptyState
        title={`${symbol} is not on your tracker`}
        description="Add it to start scoring detectors and capture alerts on this ticker."
        cta={
          openAddToTracker ? (
            <Button size="sm" onClick={() => openAddToTracker({ symbol, source: "manual" })}>
              Add to tracker
            </Button>
          ) : null
        }
      />
    )
  }

  const handleSaveTags = async (next: StrategyTag[]) => {
    setSaveError(null)
    try {
      await setTags(ticker.symbol, next)
      setEditing(false)
    } catch (err) {
      setSaveError(err instanceof Error ? err.message : String(err))
    }
  }

  return (
    <Card className="border-border bg-card/50 backdrop-blur-xs">
      <CardHeader>
        <div className="flex items-start justify-between gap-4">
          <div>
            <CardTitle className="text-foreground">Watchlist</CardTitle>
            <CardDescription className="text-muted-foreground">
              Tracker metadata for {ticker.symbol}.
            </CardDescription>
          </div>
          {activeSetup && <SetupBadge setup={activeSetup} />}
        </div>
      </CardHeader>
      <CardContent className="space-y-4 text-sm">
        {saveError && (
          <div className="rounded-md border border-rose-400/40 bg-rose-500/10 px-3 py-2 text-xs text-rose-200">
            {saveError}
          </div>
        )}

        <div className="grid grid-cols-1 gap-3 sm:grid-cols-3">
          <Field label="Status" value={STATUS_LABELS[ticker.status]} />
          <Field label="Source" value={ticker.source} />
          <Field label="Added" value={new Date(ticker.added_at).toLocaleDateString()} />
        </div>

        <div className="space-y-1.5">
          <div className="flex items-center justify-between">
            <span className="text-muted-foreground text-xs tracking-wide uppercase">Tags</span>
            {!editing && (
              <Button
                variant="ghost"
                size="sm"
                className="h-7 gap-1 text-xs"
                onClick={() => setEditing(true)}
              >
                <Pencil className="h-3 w-3" />
                Edit
              </Button>
            )}
          </div>
          {editing ? (
            <TagEditor
              tags={ticker.tags}
              onSave={handleSaveTags}
              onCancel={() => {
                setEditing(false)
                setSaveError(null)
              }}
            />
          ) : (
            <div className="flex flex-wrap gap-1">
              {ticker.tags.length === 0 ? (
                <span className="text-muted-foreground text-xs">No tags</span>
              ) : (
                ticker.tags.map((tag) => (
                  <Badge key={tag} variant="outline" className="border-input text-foreground">
                    {tagLabel(tag)}
                  </Badge>
                ))
              )}
            </div>
          )}
        </div>

        {ticker.notes && (
          <div className="space-y-1">
            <span className="text-muted-foreground text-xs tracking-wide uppercase">Notes</span>
            <p className="text-foreground/90 text-xs leading-relaxed whitespace-pre-wrap">
              {ticker.notes}
            </p>
          </div>
        )}
      </CardContent>
    </Card>
  )
}

function Field({ label, value }: { label: string; value: string }) {
  return (
    <div className="space-y-0.5">
      <span className="text-muted-foreground text-xs tracking-wide uppercase">{label}</span>
      <p className="text-foreground text-sm">{value}</p>
    </div>
  )
}
