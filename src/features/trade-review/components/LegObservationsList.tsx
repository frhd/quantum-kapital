import { useState } from "react"

import { Button } from "../../../shared/components/ui/button"
import type { LegObservation } from "../types"
import { BehavioralTagChip } from "./BehavioralTagChip"

export function LegObservationsList({ items }: { items: LegObservation[] }) {
  const [open, setOpen] = useState(false)
  if (items.length === 0) {
    return null
  }
  return (
    <section className="border-border mt-3 border-t pt-3">
      <div className="flex items-center justify-between">
        <h3 className="text-foreground text-xs font-semibold tracking-wider uppercase">
          Leg observations · {items.length}
        </h3>
        <Button
          size="sm"
          variant="ghost"
          className="h-7 px-2 text-xs"
          onClick={() => setOpen((v) => !v)}
        >
          {open ? "Hide" : "Show"}
        </Button>
      </div>
      {open && (
        <ul className="mt-2 space-y-2">
          {items.map((obs) => (
            <li
              key={obs.leg_id}
              className="border-border bg-background/40 space-y-1 rounded-md border p-2 text-xs"
            >
              <div className="flex items-center gap-2 text-[11px]">
                <span className="text-muted-foreground font-mono">{obs.leg_id}</span>
                {obs.symbol && <span className="text-foreground font-semibold">{obs.symbol}</span>}
                {obs.tag && <BehavioralTagChip tag={obs.tag} />}
              </div>
              <p className="text-foreground/90 whitespace-pre-wrap">{obs.observation_md}</p>
            </li>
          ))}
        </ul>
      )}
    </section>
  )
}
