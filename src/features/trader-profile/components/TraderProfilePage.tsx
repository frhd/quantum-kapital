/**
 * Phase 7 — Trader Profile dashboard.
 *
 * Pure SQL aggregate over `day_reviews`: tag frequencies, P&L by
 * behavioral tag, last-7d-vs-prior-21d trendline, recent leg-level
 * incidents. Read-only — the underlying aggregator runs on demand,
 * no LLM calls. Window defaults to 30 days, clamped to [1, 365] in the
 * backend.
 */

import { useState } from "react"

import { Button } from "../../../shared/components/ui/button"
import { Card, CardContent, CardHeader, CardTitle } from "../../../shared/components/ui/card"
import { Skeleton } from "../../../shared/components/ui/skeleton"

import { useTraderProfile } from "../hooks/useTraderProfile"
import { PnlByTagHeatmap } from "./PnlByTagHeatmap"
import { RecentIncidentsList } from "./RecentIncidentsList"
import { TagFrequencyChart } from "./TagFrequencyChart"
import { TrendlineCard } from "./TrendlineCard"

const WINDOW_OPTIONS: number[] = [7, 14, 30, 60, 90]

export interface TraderProfilePageProps {
  defaultWindowDays?: number
  account?: string | null
}

export function TraderProfilePage({
  defaultWindowDays = 30,
  account,
}: TraderProfilePageProps = {}) {
  const [windowDays, setWindowDays] = useState(defaultWindowDays)
  const { profile, loading, refreshing, error, refresh } = useTraderProfile(
    windowDays,
    account ?? null,
  )

  return (
    <Card className="border-border bg-card/50">
      <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
        <div>
          <CardTitle className="text-base font-semibold">Trader Profile</CardTitle>
          <p className="text-muted-foreground mt-1 text-xs">
            Behavioral history aggregated from {profile?.n_reviews ?? 0} review
            {profile?.n_reviews === 1 ? "" : "s"} since {profile?.since_date ?? "—"}.
          </p>
        </div>
        <div className="flex items-center gap-2">
          <select
            value={windowDays}
            onChange={(e) => setWindowDays(Number(e.target.value))}
            className="border-border bg-background h-8 rounded-md border px-2 text-xs"
            aria-label="Window in days"
          >
            {WINDOW_OPTIONS.map((days) => (
              <option key={days} value={days}>
                {days} days
              </option>
            ))}
          </select>
          <Button
            size="sm"
            variant="ghost"
            onClick={() => void refresh()}
            disabled={refreshing}
            className="h-8 px-3 text-xs"
          >
            {refreshing ? "Refreshing…" : "Refresh"}
          </Button>
        </div>
      </CardHeader>
      <CardContent className="space-y-4">
        {error ? (
          <p className="text-destructive text-sm">Failed to load profile: {error}</p>
        ) : loading ? (
          <div className="space-y-2">
            <Skeleton className="bg-secondary h-24" />
            <Skeleton className="bg-secondary h-32" />
          </div>
        ) : !profile || profile.n_reviews === 0 ? (
          <Card>
            <CardContent className="text-muted-foreground py-12 text-center text-sm">
              <p>Profile is empty.</p>
              <p className="text-muted-foreground/70 mt-2 text-xs">
                The system needs a few EOD reviews to learn your behavioral patterns. Reviews are
                written automatically at 17:00 ET on every trading day.
              </p>
            </CardContent>
          </Card>
        ) : (
          <>
            <TrendlineCard trendline={profile.trendline} />
            <Card>
              <CardHeader className="pb-2">
                <CardTitle className="text-sm font-semibold">Tag frequencies</CardTitle>
                <p className="text-muted-foreground text-xs">
                  How often each behavioral tag fired over the last {profile.window_days} days.
                </p>
              </CardHeader>
              <CardContent>
                <TagFrequencyChart frequencies={profile.tag_frequencies} />
              </CardContent>
            </Card>
            <Card>
              <CardHeader className="pb-2">
                <CardTitle className="text-sm font-semibold">P&L attribution</CardTitle>
                <p className="text-muted-foreground text-xs">
                  Aggregate P&L for days each tag fired.
                </p>
              </CardHeader>
              <CardContent>
                <PnlByTagHeatmap rows={profile.pnl_by_tag} />
              </CardContent>
            </Card>
            <Card>
              <CardHeader className="pb-2">
                <CardTitle className="text-sm font-semibold">
                  Recent incidents · {profile.recent_incidents.length}
                </CardTitle>
                <p className="text-muted-foreground text-xs">
                  Leg-level observations the EOD reviewer flagged in this window.
                </p>
              </CardHeader>
              <CardContent>
                <RecentIncidentsList incidents={profile.recent_incidents} />
              </CardContent>
            </Card>
          </>
        )}
      </CardContent>
    </Card>
  )
}
