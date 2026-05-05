/**
 * Phase 7 — Today's Playbook panel.
 *
 * Renders the persisted `playbooks` row for a given trading day:
 * ranked actionable setups (trigger / entry / invalidation / target /
 * conviction) plus a behavioral skip list. Distinguishes loading /
 * error / empty / populated states. Playbooks are written by
 * `agent/morning_sweep.py` at 07:00 ET via the `write_playbook` MCP
 * write rail.
 */

import { useState } from "react"

import { Button } from "../../../shared/components/ui/button"
import { Card, CardContent, CardHeader, CardTitle } from "../../../shared/components/ui/card"
import { Skeleton } from "../../../shared/components/ui/skeleton"

import { usePlaybook } from "../hooks/usePlaybook"
import type { Playbook } from "../types"
import { EmptyPlaybook } from "./EmptyPlaybook"
import { RankedSetupCard } from "./RankedSetupCard"
import { SkipListSection } from "./SkipListSection"

const ET_DATE_FMT = new Intl.DateTimeFormat("en-CA", { timeZone: "America/New_York" })

function todayEt(): string {
  return ET_DATE_FMT.format(new Date())
}

function fmtTime(iso: string): string {
  try {
    return new Date(iso).toLocaleTimeString(undefined, {
      hour: "numeric",
      minute: "2-digit",
    })
  } catch {
    return iso
  }
}

function PopulatedPlaybook({ playbook }: { playbook: Playbook }) {
  return (
    <div className="space-y-4">
      <p className="text-muted-foreground text-xs">
        generated {fmtTime(playbook.generated_at)} · gen #{playbook.generation_id}
      </p>
      {playbook.ranked_setups.length === 0 ? (
        <Card>
          <CardContent className="text-muted-foreground py-6 text-center text-sm">
            No A/B-conviction setups today.
          </CardContent>
        </Card>
      ) : (
        <div className="space-y-3" data-testid="ranked-setups">
          {playbook.ranked_setups.map((setup, i) => (
            <RankedSetupCard key={`${setup.symbol}-${i}`} setup={setup} />
          ))}
        </div>
      )}
      <SkipListSection items={playbook.skip_list} />
    </div>
  )
}

export interface TodaysPlaybookProps {
  date?: string
  account?: string | null
}

export function TodaysPlaybook({ date: dateProp, account }: TodaysPlaybookProps = {}) {
  const [date, setDate] = useState(dateProp ?? todayEt())
  const { playbook, loading, refreshing, error, refresh } = usePlaybook(date, account ?? null)

  return (
    <Card className="border-border bg-card/50">
      <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
        <div>
          <CardTitle className="text-base font-semibold">Today's Playbook</CardTitle>
          <p className="text-muted-foreground mt-1 text-xs">
            Pre-market ranked setups + behavioral skip list for {date} (ET).
          </p>
        </div>
        <div className="flex items-center gap-2">
          <input
            type="date"
            value={date}
            onChange={(e) => setDate(e.target.value || todayEt())}
            className="border-border bg-background h-8 rounded-md border px-2 text-xs"
            aria-label="Trading day"
          />
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
      <CardContent>
        {error ? (
          <p className="text-destructive text-sm">Failed to load playbook: {error}</p>
        ) : loading ? (
          <div className="space-y-2">
            <Skeleton className="bg-secondary h-16" />
            <Skeleton className="bg-secondary h-24" />
          </div>
        ) : playbook ? (
          <PopulatedPlaybook playbook={playbook} />
        ) : (
          <EmptyPlaybook date={date} />
        )}
      </CardContent>
    </Card>
  )
}
