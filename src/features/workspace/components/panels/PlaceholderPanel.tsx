import { Card, CardContent } from "../../../../shared/components/ui/card"

interface PlaceholderPanelProps {
  label: string
  phase?: number
}

export function PlaceholderPanel({ label, phase }: PlaceholderPanelProps) {
  const message = phase
    ? `${label} — coming in Phase ${phase}.`
    : `${label} — coming in a later phase.`
  return (
    <Card className="border-border bg-card/50">
      <CardContent className="py-12 text-center">
        <p className="text-muted-foreground text-sm">{message}</p>
      </CardContent>
    </Card>
  )
}
