import { Card, CardContent, CardHeader, CardTitle } from "../../../shared/components/ui/card"
import type { Trendline, WindowSummary } from "../types"

function fmtUsd(value: number): string {
  const sign = value < 0 ? "-" : ""
  const abs = Math.abs(value)
  return `${sign}$${abs.toFixed(2)}`
}

function pnlClass(value: number): string {
  if (value > 0) return "text-green-400"
  if (value < 0) return "text-red-400"
  return "text-muted-foreground"
}

function topTag(summary: WindowSummary): string {
  const entries = Object.entries(summary.tag_counts)
  if (entries.length === 0) return "—"
  entries.sort((a, b) => b[1] - a[1])
  return `${entries[0][0]} (${entries[0][1]})`
}

function WindowColumn({ label, summary }: { label: string; summary: WindowSummary }) {
  return (
    <div className="space-y-1">
      <h3 className="text-muted-foreground text-[10px] tracking-wider uppercase">{label}</h3>
      <Row label="Reviews" value={String(summary.n_reviews)} />
      <Row label="Net P&L" value={fmtUsd(summary.net_pnl)} valueClass={pnlClass(summary.net_pnl)} />
      <Row label="Avg score" value={summary.avg_grade_score.toFixed(1)} />
      <Row label="Top tag" value={topTag(summary)} />
    </div>
  )
}

function Row({ label, value, valueClass }: { label: string; value: string; valueClass?: string }) {
  return (
    <div className="grid grid-cols-[max-content_1fr] items-baseline gap-x-3 text-sm">
      <span className="text-muted-foreground text-[10px] uppercase">{label}</span>
      <span className={`font-mono tabular-nums ${valueClass ?? ""}`}>{value}</span>
    </div>
  )
}

export function TrendlineCard({ trendline }: { trendline: Trendline }) {
  return (
    <Card>
      <CardHeader className="pb-2">
        <CardTitle className="text-base font-semibold">Trendline</CardTitle>
        <p className="text-muted-foreground text-xs">Last 7 days vs prior 21 days.</p>
      </CardHeader>
      <CardContent className="grid grid-cols-2 gap-4" data-testid="trendline">
        <WindowColumn label="Last 7 days" summary={trendline.last_7d} />
        <WindowColumn label="Prior 21 days" summary={trendline.prior_21d} />
      </CardContent>
    </Card>
  )
}
