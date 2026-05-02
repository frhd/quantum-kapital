import { Activity, Loader2 } from "lucide-react"

import { Card, CardContent, CardHeader, CardTitle } from "../../../shared/components/ui/card"
import { useEvalDashboard } from "../hooks/useEvalDashboard"
import type { CalibrationStats, ConvictionBucket, CostAttribution } from "../types"

const WINDOW_OPTIONS = [7, 30, 90] as const

/**
 * Phase 8 — calibration + cost dashboard.
 *
 * Headline numbers answer "is the agent net positive?": per-conviction
 * win rate (target + entry over scoreable) and dollars spent per A-call.
 * First 30 trading days the agent is live this is mostly a "no data
 * yet" state — copy says so loudly.
 */
export function EvalTab() {
  const { calibration, cost, loading, error, windowDays, setWindowDays } = useEvalDashboard()

  return (
    <div className="space-y-4">
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Activity className="h-5 w-5" />
            Calibration & Cost
          </CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="flex items-center gap-2">
            <span className="text-muted-foreground text-sm">Window:</span>
            {WINDOW_OPTIONS.map((days) => (
              <button
                key={days}
                onClick={() => setWindowDays(days)}
                className={`rounded border px-2 py-1 text-xs ${
                  windowDays === days
                    ? "border-foreground bg-muted"
                    : "border-border text-muted-foreground hover:bg-muted/50"
                }`}
              >
                {days}d
              </button>
            ))}
            {loading && <Loader2 className="text-muted-foreground h-4 w-4 animate-spin" />}
          </div>

          {error && <p className="text-destructive text-sm">{error}</p>}

          {calibration && <CalibrationCard stats={calibration} />}
          {cost && <CostCard stats={cost} />}

          <p className="text-muted-foreground text-xs">
            Heuristic: A-conviction calls should outperform B and C on `target_rate` once you have
            ~30 trading days of data. Until then, treat the table as "no signal yet" and resist
            tuning the prompt on small samples.
          </p>
        </CardContent>
      </Card>
    </div>
  )
}

function CalibrationCard({ stats }: { stats: CalibrationStats }) {
  const rows = [...stats.buckets, stats.overall]
  return (
    <section>
      <h3 className="mb-2 text-sm font-semibold">Calibration ({stats.window_days}d)</h3>
      {stats.overall.total === 0 ? (
        <p className="text-muted-foreground text-sm">
          No scored predictions in this window. Run the morning sweep + EOD review for at least one
          trading day to see numbers here.
        </p>
      ) : (
        <table className="w-full text-sm">
          <thead className="text-muted-foreground border-border border-b text-xs uppercase">
            <tr>
              <th className="px-2 py-1 text-left font-normal">Conviction</th>
              <th className="px-2 py-1 text-right font-normal">Total</th>
              <th className="px-2 py-1 text-right font-normal">Target</th>
              <th className="px-2 py-1 text-right font-normal">Entry</th>
              <th className="px-2 py-1 text-right font-normal">Invalidation</th>
              <th className="px-2 py-1 text-right font-normal">Drift / NoMv</th>
              <th className="px-2 py-1 text-right font-normal">Skip / Unp</th>
              <th className="px-2 py-1 text-right font-normal">Win %</th>
              <th className="px-2 py-1 text-right font-normal">Target %</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((b) => (
              <ConvictionRow key={bucketKey(b)} bucket={b} />
            ))}
          </tbody>
        </table>
      )}
    </section>
  )
}

function bucketKey(b: ConvictionBucket): string {
  return b.conviction ?? "ungraded"
}

function ConvictionRow({ bucket }: { bucket: ConvictionBucket }) {
  const label = bucket.conviction ?? "—"
  return (
    <tr className="border-border border-b last:border-b-0">
      <td className="px-2 py-1 font-mono">{label}</td>
      <td className="px-2 py-1 text-right">{bucket.total}</td>
      <td className="px-2 py-1 text-right">{bucket.hit_target}</td>
      <td className="px-2 py-1 text-right">{bucket.hit_entry}</td>
      <td className="px-2 py-1 text-right">{bucket.hit_invalidation}</td>
      <td className="px-2 py-1 text-right">
        {bucket.drifted} / {bucket.no_movement}
      </td>
      <td className="px-2 py-1 text-right">
        {bucket.skipped} / {bucket.unparseable}
      </td>
      <td className="px-2 py-1 text-right">
        {formatRate(bucket.win_rate, bucket.total - bucket.skipped - bucket.unparseable)}
      </td>
      <td className="px-2 py-1 text-right">
        {formatRate(bucket.target_rate, bucket.total - bucket.skipped - bucket.unparseable)}
      </td>
    </tr>
  )
}

function formatRate(rate: number, scoreable: number): string {
  if (scoreable <= 0) return "—"
  return `${(rate * 100).toFixed(1)}%`
}

function CostCard({ stats }: { stats: CostAttribution }) {
  return (
    <section>
      <h3 className="mb-2 text-sm font-semibold">Cost attribution ({stats.window_days}d)</h3>
      <div className="text-muted-foreground mb-2 text-xs">
        Total: ${stats.total_cost_usd.toFixed(2)} across {stats.total_calls} calls ·{" "}
        {stats.a_conviction_count} A-conviction predictions ·{" "}
        {stats.usd_per_a_conviction == null
          ? "$/A-call: —"
          : `$${stats.usd_per_a_conviction.toFixed(2)} per A-call`}
      </div>
      {stats.buckets.length === 0 ? (
        <p className="text-muted-foreground text-sm">No LLM calls in window.</p>
      ) : (
        <table className="w-full text-sm">
          <thead className="text-muted-foreground border-border border-b text-xs uppercase">
            <tr>
              <th className="px-2 py-1 text-left font-normal">Bucket</th>
              <th className="px-2 py-1 text-right font-normal">Calls</th>
              <th className="px-2 py-1 text-right font-normal">USD</th>
            </tr>
          </thead>
          <tbody>
            {stats.buckets.map((b) => (
              <tr key={b.bucket} className="border-border border-b last:border-b-0">
                <td className="px-2 py-1 font-mono">{b.bucket}</td>
                <td className="px-2 py-1 text-right">{b.call_count}</td>
                <td className="px-2 py-1 text-right">${b.cost_usd.toFixed(4)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </section>
  )
}
