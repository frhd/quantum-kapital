import { Card, CardContent, CardHeader, CardTitle } from "../../../shared/components/ui/card"
import type { SkipEntry } from "../types"

export function SkipListSection({ items }: { items: SkipEntry[] }) {
  if (items.length === 0) return null
  return (
    <Card className="border-border bg-card/30">
      <CardHeader className="pb-2">
        <CardTitle className="text-foreground text-sm font-semibold tracking-wider uppercase">
          Skip list · {items.length}
        </CardTitle>
      </CardHeader>
      <CardContent>
        <ul className="divide-border divide-y" data-testid="skip-list">
          {items.map((entry) => (
            <li
              key={entry.symbol}
              className="grid grid-cols-[max-content_1fr] gap-x-3 py-1.5 text-sm"
            >
              <span className="text-foreground font-mono font-semibold">{entry.symbol}</span>
              <span className="text-muted-foreground">{entry.reason}</span>
            </li>
          ))}
        </ul>
      </CardContent>
    </Card>
  )
}
