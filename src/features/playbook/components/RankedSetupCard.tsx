import { Card, CardContent, CardHeader, CardTitle } from "../../../shared/components/ui/card"

import { MarkdownBody } from "../../research/components/MarkdownBody"
import type { Conviction, RankedSetup } from "../types"

const CONVICTION_CLASSES: Record<Conviction, string> = {
  A: "bg-emerald-500/20 text-emerald-300 border-emerald-500/40",
  B: "bg-amber-500/15 text-amber-300 border-amber-500/40",
  C: "bg-secondary text-foreground/80 border-border",
}

function ConvictionBadge({ conviction }: { conviction: Conviction }) {
  return (
    <span
      className={`inline-flex h-6 w-6 items-center justify-center rounded-md border font-mono text-xs font-semibold ${CONVICTION_CLASSES[conviction]}`}
      title={`${conviction}-conviction`}
      data-testid="conviction-badge"
    >
      {conviction}
    </span>
  )
}

function BiasBadge({ bias }: { bias: "long" | "short" }) {
  const cls =
    bias === "long"
      ? "bg-green-500/15 text-green-300 border-green-500/30"
      : "bg-red-500/15 text-red-300 border-red-500/30"
  return (
    <span className={`rounded-sm border px-1.5 py-0.5 font-mono text-[10px] uppercase ${cls}`}>
      {bias}
    </span>
  )
}

function Row({ label, value, valueClass }: { label: string; value: string; valueClass?: string }) {
  return (
    <div className="grid grid-cols-[max-content_1fr] gap-x-3 text-sm">
      <span className="text-muted-foreground text-[10px] tracking-wider uppercase">{label}</span>
      <span className={`text-foreground/90 ${valueClass ?? ""}`}>{value}</span>
    </div>
  )
}

export function RankedSetupCard({ setup }: { setup: RankedSetup }) {
  return (
    <Card className="border-border bg-card/50">
      <CardHeader className="pb-2">
        <CardTitle className="flex items-center gap-2 text-base font-semibold">
          <ConvictionBadge conviction={setup.conviction} />
          <span>{setup.symbol}</span>
          <BiasBadge bias={setup.bias} />
        </CardTitle>
      </CardHeader>
      <CardContent className="space-y-2">
        <Row label="Trigger" value={setup.trigger} />
        <Row label="Entry" value={setup.entry} />
        <Row label="Invalidation" value={setup.invalidation} valueClass="text-red-300/90" />
        <Row label="Target 1" value={setup.target_1} />
        {setup.target_2 && <Row label="Target 2" value={setup.target_2} />}
        {setup.rationale_md.trim().length > 0 && <MarkdownBody markdown={setup.rationale_md} />}
        {setup.evidence_refs.length > 0 && (
          <ul className="text-muted-foreground border-border mt-2 space-y-0.5 border-t pt-2 text-[11px]">
            {setup.evidence_refs.map((ref, i) => (
              <li key={`${ref.source}-${i}`}>
                <span className="font-mono uppercase">{ref.source}</span> · {ref.note}
              </li>
            ))}
          </ul>
        )}
      </CardContent>
    </Card>
  )
}
