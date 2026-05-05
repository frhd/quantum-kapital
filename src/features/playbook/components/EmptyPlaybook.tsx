import { Card, CardContent } from "../../../shared/components/ui/card"

export function EmptyPlaybook({ date }: { date: string }) {
  return (
    <Card>
      <CardContent className="text-muted-foreground py-12 text-center text-sm">
        <p>No playbook for {date} yet.</p>
        <p className="text-muted-foreground/70 mt-2 text-xs">
          Playbooks are generated automatically at 07:00 ET. Check back before the open, or run{" "}
          <code className="bg-muted rounded px-1 py-0.5 font-mono text-xs">
            uv run qk-morning-sweep --date {date}
          </code>{" "}
          manually.
        </p>
      </CardContent>
    </Card>
  )
}
