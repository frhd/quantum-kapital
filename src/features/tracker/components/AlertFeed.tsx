import { useState } from "react"
import { Bell, CheckCheck, RefreshCw } from "lucide-react"
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "../../../shared/components/ui/card"
import { Button } from "../../../shared/components/ui/button"
import { useAlerts } from "../hooks/useAlerts"
import { AlertRow } from "./AlertRow"
import { ALERT_KIND_LABELS, type AlertKind } from "../types"
import type {
  SetupDetectedPayload,
  SetupInvalidatedPayload,
  TickerStatusChangedPayload,
} from "../types"
import { useTickerNavigate } from "../../workspace/hooks/useTickerNavigate"

interface AlertFeedProps {
  lastSetupDetected: SetupDetectedPayload | null
  lastInvalidated: SetupInvalidatedPayload | null
  lastStatusChanged: TickerStatusChangedPayload | null
}

type KindFilter = "all" | AlertKind

const KIND_OPTIONS: ReadonlyArray<{ value: KindFilter; label: string }> = [
  { value: "all", label: "All" },
  { value: "detected", label: ALERT_KIND_LABELS.detected },
  { value: "invalidated", label: ALERT_KIND_LABELS.invalidated },
  { value: "target_hit", label: ALERT_KIND_LABELS.target_hit },
  { value: "thesis_changed", label: ALERT_KIND_LABELS.thesis_changed },
]

export function AlertFeed({
  lastSetupDetected,
  lastInvalidated,
  lastStatusChanged,
}: AlertFeedProps) {
  const navigate = useTickerNavigate()
  const [kindFilter, setKindFilter] = useState<KindFilter>("all")
  const [onlyUnseen, setOnlyUnseen] = useState(false)

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
    filterKind: kindFilter === "all" ? null : kindFilter,
    onlyUnseen,
  })

  const handleClick = (id: number, symbol: string | undefined) => {
    void markOneSeen(id)
    if (symbol) navigate(symbol, "alerts")
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
                Recent setup events. Click a row to open the analysis.
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
        <div className="flex flex-wrap items-center gap-2">
          <span className="text-muted-foreground text-xs">Kind:</span>
          {KIND_OPTIONS.map((opt) => {
            const active = kindFilter === opt.value
            return (
              <button
                key={opt.value}
                type="button"
                onClick={() => setKindFilter(opt.value)}
                className={
                  "rounded-full border px-3 py-0.5 text-xs transition-colors " +
                  (active
                    ? "border-blue-400 bg-blue-500/20 text-blue-100"
                    : "border-input bg-card text-foreground hover:bg-secondary")
                }
              >
                {opt.label}
              </button>
            )
          })}
          <label className="text-muted-foreground ml-auto flex items-center gap-1.5 text-xs">
            <input
              type="checkbox"
              checked={onlyUnseen}
              onChange={(e) => setOnlyUnseen(e.target.checked)}
              className="border-border bg-secondary size-3.5 rounded text-blue-500"
            />
            Unseen only
          </label>
        </div>

        {error && (
          <div className="rounded-md border border-rose-400/40 bg-rose-500/10 px-3 py-2 text-xs text-rose-200">
            {error}
          </div>
        )}

        {alerts.length === 0 && !loading ? (
          <p className="text-muted-foreground py-6 text-center text-sm">
            No alerts to show yet. Detector hits and invalidations will land here.
          </p>
        ) : (
          <div className="flex max-h-[28rem] flex-col gap-1.5 overflow-y-auto">
            {alerts.map((a) => (
              <AlertRow
                key={a.id}
                alert={a}
                onClick={() =>
                  handleClick(
                    a.id,
                    typeof a.payload.symbol === "string" ? a.payload.symbol : undefined,
                  )
                }
              />
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
