import { Bell, CheckCheck, Loader2, RefreshCw } from "lucide-react"
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "../../../../shared/components/ui/card"
import { Button } from "../../../../shared/components/ui/button"
import { AlertRow } from "../../../tracker/components/AlertRow"
import { useAlerts } from "../../../tracker/hooks/useAlerts"
import { useTrackerEvents } from "../../../tracker/hooks/useTrackerEvents"
import { useWorkspace } from "../../context/WorkspaceContext"
import { EmptyState } from "../EmptyState"

/**
 * Workspace Phase 2 — alerts panel scoped to the active symbol. Uses
 * the existing `useAlerts` hook with the new `symbol` arg so server-
 * side pagination stays correct. The kind filter / unseen toggle from
 * the global feed are intentionally dropped — by the time the user is
 * in this panel the feed is already filtered to one symbol, which is
 * the heaviest cut, so stacking more filters mostly gets in the way.
 */
export function AlertsPanel() {
  const { symbol } = useWorkspace()
  const { lastSetupDetected, lastInvalidated, lastStatusChanged } = useTrackerEvents()
  const {
    alerts,
    loading,
    error,
    unseenCount,
    hasMore,
    refresh,
    loadMore,
    markAllSeen,
    markOneSeen,
  } = useAlerts({
    lastSetupDetected,
    lastInvalidated,
    lastStatusChanged,
    filterKind: null,
    onlyUnseen: false,
    symbol: symbol ?? null,
  })

  if (!symbol) {
    return (
      <EmptyState
        title="No symbol selected"
        description="Search for a ticker above to load its alert history."
      />
    )
  }

  return (
    <Card className="border-border bg-card/50 backdrop-blur-xs">
      <CardHeader>
        <div className="flex items-center justify-between gap-4">
          <div className="flex items-center gap-2">
            <Bell className="h-4 w-4 text-blue-300" />
            <div>
              <CardTitle className="text-foreground">Alerts</CardTitle>
              <CardDescription className="text-muted-foreground">
                Setup events fired for {symbol}.
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
              onClick={() => void markAllSeen()}
              disabled={unseenCount === 0}
              className="h-8 gap-1"
              title="Mark all as seen"
            >
              <CheckCheck className="h-4 w-4" />
              Mark all seen
              {unseenCount > 0 && (
                <span className="ml-1 rounded-full bg-blue-500/20 px-1.5 text-[10px] text-blue-200">
                  {unseenCount}
                </span>
              )}
            </Button>
          </div>
        </div>
      </CardHeader>
      <CardContent className="space-y-3">
        {error && (
          <div className="rounded-md border border-rose-400/40 bg-rose-500/10 px-3 py-2 text-xs text-rose-200">
            {error}
          </div>
        )}

        {alerts.length === 0 && !loading && !error ? (
          <EmptyState
            title={`No alerts for ${symbol} yet`}
            description="Detector hits, invalidations, target hits, and thesis updates will land here."
          />
        ) : alerts.length === 0 && loading ? (
          <div className="text-muted-foreground flex items-center justify-center gap-2 py-6 text-xs">
            <Loader2 className="h-4 w-4 animate-spin" />
            Loading alerts…
          </div>
        ) : (
          <div className="flex max-h-[28rem] flex-col gap-1.5 overflow-y-auto">
            {alerts.map((a) => (
              <AlertRow key={a.id} alert={a} onClick={() => void markOneSeen(a.id)} />
            ))}
            {hasMore && (
              <Button
                variant="outline"
                size="sm"
                onClick={() => void loadMore()}
                disabled={loading}
                className="mt-2 self-center"
              >
                Load more
              </Button>
            )}
          </div>
        )}
      </CardContent>
    </Card>
  )
}
