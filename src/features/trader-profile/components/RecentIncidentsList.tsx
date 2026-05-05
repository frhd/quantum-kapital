import { BehavioralTagChip } from "../../trade-review/components/BehavioralTagChip"
import type { RecentIncident } from "../types"

export function RecentIncidentsList({ incidents }: { incidents: RecentIncident[] }) {
  if (incidents.length === 0) {
    return (
      <p className="text-muted-foreground py-4 text-center text-xs">
        No recent incidents in this window.
      </p>
    )
  }
  return (
    <ul className="space-y-2" data-testid="recent-incidents">
      {incidents.map((incident, i) => (
        <li
          key={`${incident.date}-${incident.symbol}-${incident.tag}-${i}`}
          className="border-border bg-background/40 space-y-1 rounded-md border p-2 text-xs"
        >
          <div className="flex items-center gap-2 text-[11px]">
            <span className="text-muted-foreground font-mono">{incident.date}</span>
            <span className="text-foreground font-semibold">{incident.symbol}</span>
            <BehavioralTagChip tag={incident.tag} />
          </div>
          <p className="text-foreground/90 whitespace-pre-wrap">{incident.leg_observation}</p>
        </li>
      ))}
    </ul>
  )
}
