import { Card, CardContent } from "../../../shared/components/ui/card"

export function EmptyTradeReview({ date }: { date: string }) {
  return (
    <Card>
      <CardContent className="text-muted-foreground py-12 text-center text-sm">
        <p>No trade review for {date} yet.</p>
        <p className="text-muted-foreground/70 mt-2 text-xs">
          Reviews are written automatically at 17:00 ET. Check back after market close, or run{" "}
          <code className="bg-muted rounded px-1 py-0.5 font-mono text-xs">
            uv run qk-eod-review --date {date}
          </code>{" "}
          manually.
        </p>
      </CardContent>
    </Card>
  )
}
