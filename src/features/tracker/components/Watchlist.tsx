import { useState } from "react"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "../../../shared/components/ui/table"
import { Skeleton } from "../../../shared/components/ui/skeleton"
import { Alert, AlertDescription } from "../../../shared/components/ui/alert"
import { Badge } from "../../../shared/components/ui/badge"
import { Button } from "../../../shared/components/ui/button"
import { AlertCircle, ExternalLink, Tag, Trash2 } from "lucide-react"
import {
  BUILT_IN_TAGS,
  STATUS_LABELS,
  type Setup,
  type StrategyTag,
  type TrackedTicker,
} from "../types"
import { SetupBadge } from "./SetupBadge"
import { TagEditor } from "./TagEditor"
import { useTickerNavigate } from "../../workspace/hooks/useTickerNavigate"

interface WatchlistProps {
  tickers: TrackedTicker[]
  loading: boolean
  error: string | null
  onRemove: (symbol: string) => Promise<void> | void
  onSaveTags: (symbol: string, tags: StrategyTag[]) => Promise<unknown> | void
  activeSetupBySymbol?: Record<string, Setup>
}

function formatRelativeTime(iso: string): string {
  const ts = new Date(iso).getTime()
  if (Number.isNaN(ts)) return "—"
  const diffMs = Date.now() - ts
  const diffSec = Math.round(diffMs / 1000)
  if (diffSec < 60) return `${diffSec}s ago`
  const diffMin = Math.round(diffSec / 60)
  if (diffMin < 60) return `${diffMin}m ago`
  const diffHr = Math.round(diffMin / 60)
  if (diffHr < 24) return `${diffHr}h ago`
  const diffDay = Math.round(diffHr / 24)
  if (diffDay < 30) return `${diffDay}d ago`
  return new Date(iso).toLocaleDateString()
}

function tagLabel(tag: StrategyTag): string {
  const builtin = BUILT_IN_TAGS.find((b) => b.value === tag)
  return builtin ? builtin.label : tag
}

function truncateThesis(s: string, max: number): string {
  if (s.length <= max) return s
  return s.slice(0, max - 1).trimEnd() + "…"
}

export function Watchlist({
  tickers,
  loading,
  error,
  onRemove,
  onSaveTags,
  activeSetupBySymbol,
}: WatchlistProps) {
  const navigate = useTickerNavigate()
  const [editingSymbol, setEditingSymbol] = useState<string | null>(null)
  const [removingSymbol, setRemovingSymbol] = useState<string | null>(null)
  const [rowError, setRowError] = useState<string | null>(null)

  const startEditing = (ticker: TrackedTicker) => {
    setEditingSymbol(ticker.symbol)
    setRowError(null)
  }

  const cancelEditing = () => {
    setEditingSymbol(null)
  }

  const saveTags = async (symbol: string, next: StrategyTag[]) => {
    setRowError(null)
    try {
      await onSaveTags(symbol, next)
      setEditingSymbol(null)
    } catch (err) {
      setRowError(err instanceof Error ? err.message : String(err))
    }
  }

  const handleRemove = async (symbol: string) => {
    setRemovingSymbol(symbol)
    setRowError(null)
    try {
      await onRemove(symbol)
    } catch (err) {
      setRowError(err instanceof Error ? err.message : String(err))
    } finally {
      setRemovingSymbol(null)
    }
  }

  if (error) {
    return (
      <Alert variant="destructive">
        <AlertCircle className="h-4 w-4" />
        <AlertDescription>{error}</AlertDescription>
      </Alert>
    )
  }

  if (loading) {
    return (
      <div className="space-y-2">
        <Skeleton className="bg-secondary/50 h-10 w-full" />
        <Skeleton className="bg-secondary/50 h-10 w-full" />
        <Skeleton className="bg-secondary/50 h-10 w-full" />
      </div>
    )
  }

  if (tickers.length === 0) {
    return (
      <p className="text-muted-foreground py-12 text-center text-sm">
        No tickers tracked yet. Click <span className="font-medium">Add</span> or use the
        scanner&apos;s <span className="font-medium">Add to tracker</span> button to start.
      </p>
    )
  }

  return (
    <div className="space-y-2">
      {rowError && (
        <Alert variant="destructive">
          <AlertCircle className="h-4 w-4" />
          <AlertDescription>{rowError}</AlertDescription>
        </Alert>
      )}
      <div className="overflow-x-auto">
        <Table>
          <TableHeader>
            <TableRow className="border-border">
              <TableHead className="text-foreground text-xs">Symbol</TableHead>
              <TableHead className="text-foreground text-xs">Tags</TableHead>
              <TableHead className="text-foreground text-xs">Source</TableHead>
              <TableHead className="text-foreground text-xs">Status</TableHead>
              <TableHead className="text-foreground text-xs">Added</TableHead>
              <TableHead className="text-foreground text-right text-xs">Actions</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {tickers.map((t) => {
              const isEditing = editingSymbol === t.symbol
              const isRemoving = removingSymbol === t.symbol
              const activeSetup = activeSetupBySymbol?.[t.symbol]
              return (
                <TableRow key={t.symbol} className="border-border">
                  <TableCell className="text-foreground font-medium">
                    <div className="flex flex-col gap-1">
                      <span>{t.symbol}</span>
                      {activeSetup && <SetupBadge setup={activeSetup} />}
                      {activeSetup?.thesis && (
                        <p
                          className="text-foreground max-w-md text-xs leading-snug"
                          title={activeSetup.thesis}
                        >
                          {truncateThesis(activeSetup.thesis, 180)}
                        </p>
                      )}
                    </div>
                  </TableCell>
                  <TableCell>
                    {isEditing ? (
                      <TagEditor
                        tags={t.tags}
                        onSave={(next) => saveTags(t.symbol, next)}
                        onCancel={cancelEditing}
                      />
                    ) : (
                      <div className="flex flex-wrap gap-1">
                        {t.tags.length === 0 ? (
                          <span className="text-muted-foreground text-xs">—</span>
                        ) : (
                          t.tags.map((tag) => (
                            <Badge
                              key={tag}
                              variant="outline"
                              className="border-input text-foreground"
                            >
                              {tagLabel(tag)}
                            </Badge>
                          ))
                        )}
                      </div>
                    )}
                  </TableCell>
                  <TableCell className="text-foreground text-sm">{t.source}</TableCell>
                  <TableCell className="text-foreground text-sm">
                    {STATUS_LABELS[t.status]}
                  </TableCell>
                  <TableCell className="text-muted-foreground text-sm">
                    {formatRelativeTime(t.added_at)}
                  </TableCell>
                  <TableCell className="text-right">
                    {!isEditing && (
                      <div className="flex justify-end gap-1">
                        <Button
                          variant="ghost"
                          size="sm"
                          className="text-foreground hover:text-foreground h-8 px-2"
                          onClick={() => navigate(t.symbol, "overview")}
                          title="Open in workspace"
                        >
                          <ExternalLink className="h-4 w-4" />
                        </Button>
                        <Button
                          variant="ghost"
                          size="sm"
                          className="text-foreground hover:text-foreground h-8 px-2"
                          onClick={() => startEditing(t)}
                          title="Edit tags"
                        >
                          <Tag className="h-4 w-4" />
                        </Button>
                        <Button
                          variant="ghost"
                          size="sm"
                          className="h-8 px-2 text-red-400 hover:text-red-300"
                          onClick={() => handleRemove(t.symbol)}
                          disabled={isRemoving}
                          title="Remove"
                        >
                          <Trash2 className="h-4 w-4" />
                        </Button>
                      </div>
                    )}
                  </TableCell>
                </TableRow>
              )
            })}
          </TableBody>
        </Table>
      </div>
    </div>
  )
}
