/**
 * Phase 4 — Candidate-universe browser.
 *
 * Renders the agent inbox: scanner + sentiment-surge hits that
 * haven't been promoted into the watchlist yet. Lets the user
 * filter by source, score floor, and toggle the "include promoted"
 * audit view; promote rows manually with a reason.
 */

import { useMemo, useState } from "react"
import { Card, CardContent, CardHeader, CardTitle } from "../../../shared/components/ui/card"
import { Button } from "../../../shared/components/ui/button"
import { Input } from "../../../shared/components/ui/input"
import { Badge } from "../../../shared/components/ui/badge"
import { Skeleton } from "../../../shared/components/ui/skeleton"
import { ibkrApi } from "../../../shared/api/ibkr"
import { useCandidates } from "../hooks/useCandidates"
import { useTickerNavigate } from "../../workspace/hooks/useTickerNavigate"
import type { Candidate } from "../types"

export function CandidateBrowser() {
  const [sourceFilter, setSourceFilter] = useState("")
  const [minScore, setMinScore] = useState<number>(0)
  const [includePromoted, setIncludePromoted] = useState(false)

  const query = useMemo(
    () => ({
      source: sourceFilter.trim() ? sourceFilter.trim() : null,
      min_score: minScore > 0 ? minScore : null,
      include_promoted: includePromoted,
      limit: 100,
    }),
    [sourceFilter, minScore, includePromoted],
  )

  const { candidates, loading, error, refresh, triggerRefresh, refreshing } = useCandidates(query)

  return (
    <Card className="border-border bg-card/50">
      <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
        <div>
          <CardTitle className="text-base font-semibold">Candidate universe</CardTitle>
          <p className="text-muted-foreground mt-1 text-xs">
            Staging layer for scanner + sentiment-surge hits. Promote manually or wait for the
            agent.
          </p>
        </div>
        <div className="flex items-center gap-2">
          <Button
            size="sm"
            variant="ghost"
            onClick={() => void refresh()}
            disabled={loading}
            className="h-8 px-3 text-xs"
          >
            Reload
          </Button>
          <Button
            size="sm"
            variant="default"
            onClick={() => void triggerRefresh()}
            disabled={refreshing}
            className="h-8 px-3 text-xs"
          >
            {refreshing ? "Refreshing…" : "Refresh now"}
          </Button>
        </div>
      </CardHeader>
      <CardContent className="space-y-4">
        <div className="flex flex-wrap items-center gap-3">
          <Input
            value={sourceFilter}
            onChange={(e) => setSourceFilter(e.target.value)}
            placeholder="filter source (e.g. sentiment, top_perc_gain)"
            className="h-8 max-w-xs text-xs"
          />
          <label className="text-muted-foreground flex items-center gap-2 text-xs">
            min score
            <Input
              type="number"
              step="0.05"
              min={0}
              max={1}
              value={minScore}
              onChange={(e) => setMinScore(Number(e.target.value) || 0)}
              className="h-8 w-20 text-xs"
            />
          </label>
          <label className="text-muted-foreground flex items-center gap-2 text-xs">
            <input
              type="checkbox"
              checked={includePromoted}
              onChange={(e) => setIncludePromoted(e.target.checked)}
              className="h-3 w-3"
            />
            include promoted
          </label>
        </div>

        {error ? (
          <p className="text-destructive text-sm">Failed to load candidates: {error}</p>
        ) : loading && !candidates.length ? (
          <div className="space-y-2">
            {[0, 1, 2].map((i) => (
              <Skeleton key={i} className="bg-secondary h-16" />
            ))}
          </div>
        ) : candidates.length === 0 ? (
          <p className="text-muted-foreground text-sm">
            No candidates match the current filters. The auto-scanner + sentiment-surge will
            populate this on their next ticks.
          </p>
        ) : (
          <div className="space-y-2">
            {candidates.map((c) => (
              <CandidateRow key={c.symbol} candidate={c} onPromoted={() => void refresh()} />
            ))}
          </div>
        )}
      </CardContent>
    </Card>
  )
}

function CandidateRow({ candidate, onPromoted }: { candidate: Candidate; onPromoted: () => void }) {
  const navigate = useTickerNavigate()
  const [reason, setReason] = useState("")
  const [submitting, setSubmitting] = useState(false)
  const [err, setErr] = useState<string | null>(null)
  const isPromoted = candidate.promoted_at !== null

  const onPromote = async () => {
    if (!reason.trim()) {
      setErr("reason required")
      return
    }
    setSubmitting(true)
    setErr(null)
    try {
      await ibkrApi.candidates.promote(candidate.symbol, reason.trim())
      onPromoted()
    } catch (e) {
      setErr(typeof e === "string" ? e : (e as Error).message)
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <div className="border-border bg-background/40 rounded-md border p-3">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="flex items-center gap-3">
          <button
            type="button"
            onClick={() => navigate(candidate.symbol, "overview")}
            className="text-base font-semibold underline-offset-2 hover:underline"
            title="Open in workspace"
          >
            {candidate.symbol}
          </button>
          <Badge variant="outline" className="text-xs">
            score {candidate.score.toFixed(2)}
          </Badge>
          {isPromoted && (
            <Badge variant="secondary" className="text-xs">
              promoted {formatRelative(candidate.promoted_at!)}
            </Badge>
          )}
          <span className="text-muted-foreground text-xs">
            seen {formatRelative(candidate.last_seen)}
          </span>
        </div>
        <div className="flex flex-wrap gap-1">
          {candidate.sources.map((s) => (
            <Badge key={s.source} variant="outline" className="text-[10px]">
              {s.source}
              {s.rank !== null ? ` #${s.rank}` : ""} · {s.score.toFixed(2)}
            </Badge>
          ))}
        </div>
      </div>
      {candidate.reason_md && (
        <p className="text-muted-foreground mt-2 text-xs italic">{candidate.reason_md}</p>
      )}
      {!isPromoted && (
        <div className="mt-2 flex flex-wrap items-center gap-2">
          <Input
            value={reason}
            onChange={(e) => setReason(e.target.value)}
            placeholder="reason for promoting (becomes the watchlist note)"
            className="h-8 max-w-md flex-1 text-xs"
          />
          <Button
            size="sm"
            variant="default"
            onClick={() => void onPromote()}
            disabled={submitting}
            className="h-8 px-3 text-xs"
          >
            {submitting ? "Promoting…" : "Promote"}
          </Button>
        </div>
      )}
      {err && <p className="text-destructive mt-1 text-xs">{err}</p>}
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
