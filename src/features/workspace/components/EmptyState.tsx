import type { ReactNode } from "react"
import { Card, CardContent } from "../../../shared/components/ui/card"

interface EmptyStateProps {
  title: string
  description?: string
  /** Optional CTA button (or any inline node) rendered below the description. */
  cta?: ReactNode
}

/**
 * Workspace Phase 2 — single empty-state surface for every per-symbol
 * panel. Keeping the visual treatment identical across Research /
 * Alerts / Watchlist-meta / News / History prevents the "seven empty
 * UIs" drift the master plan calls out as a risk.
 */
export function EmptyState({ title, description, cta }: EmptyStateProps) {
  return (
    <Card className="border-border bg-card/50">
      <CardContent className="flex flex-col items-center gap-2 py-10 text-center">
        <p className="text-foreground text-sm font-medium">{title}</p>
        {description && (
          <p className="text-muted-foreground max-w-md text-xs">{description}</p>
        )}
        {cta && <div className="mt-2">{cta}</div>}
      </CardContent>
    </Card>
  )
}
