import { useQuote, type QuoteError } from "../../analysis/hooks/useQuote"
import { relativeTime } from "../../../shared/lib/relativeTime"
import type { InvalidationKind, NoteTarget, ResearchNote } from "../types"

interface NoteValidityCardProps {
  note: ResearchNote
}

type ValidityStatus = "intact" | "near_invalidation" | "invalidated" | "target_hit" | "unknown"

interface ValidityResult {
  status: ValidityStatus
  /** When `target_hit`, the highest target reached. */
  hitTarget?: NoteTarget
  /** Signed % buffer to invalidation (positive = safe, negative = breached).
   *  `null` when invalidation is not set. */
  buffer?: number | null
}

/** Within 2% of invalidation = "danger zone". */
const NEAR_INVALIDATION_THRESHOLD = 0.02

function isBreached(price: number, level: number, kind: InvalidationKind): boolean {
  switch (kind) {
    // v1: intraday_breach evaluates exactly like close_below for long
    // bias. Refining to the day's low/high is a follow-up once we wire
    // a daily-bar lookup.
    case "close_below":
    case "intraday_breach":
      return price <= level
    case "close_above":
      return price >= level
  }
}

function classify(live: number | null | undefined, note: ResearchNote): ValidityResult {
  if (live == null || note.invalidation_price == null || !note.invalidation_kind) {
    return { status: "unknown" }
  }
  const breached = isBreached(live, note.invalidation_price, note.invalidation_kind)
  if (breached) {
    return {
      status: "invalidated",
      buffer: (live - note.invalidation_price) / live,
    }
  }
  // Long-bias targets: a long thesis hits T_n when price >= T_n.price.
  // Short-bias (close_above invalidation): hits when price <= T_n.price.
  // Pick the highest-rank target reached (last in array since order = T1, T2, T3 ...).
  const longBias = note.invalidation_kind !== "close_above"
  const targets = note.targets ?? []
  const hits = targets.filter((t) => (longBias ? live >= t.price : live <= t.price))
  if (hits.length > 0) {
    return {
      status: "target_hit",
      hitTarget: hits[hits.length - 1],
      buffer: (live - note.invalidation_price) / live,
    }
  }
  const buffer = (live - note.invalidation_price) / live
  if (Math.abs(buffer) < NEAR_INVALIDATION_THRESHOLD) {
    return { status: "near_invalidation", buffer }
  }
  return { status: "intact", buffer }
}

const STATUS_PILL: Record<ValidityStatus, { label: string; cls: string; title: string }> = {
  intact: {
    label: "Intact",
    cls: "border-green-500/30 bg-green-500/10 text-green-400",
    title: "Live price is on the safe side of invalidation",
  },
  near_invalidation: {
    label: "Near invalidation",
    cls: "border-amber-500/30 bg-amber-500/10 text-amber-300",
    title: "Live price is within 2% of the invalidation level",
  },
  invalidated: {
    label: "Invalidated",
    cls: "border-rose-400/60 bg-red-500/10 text-rose-300",
    title: "Live price has breached the invalidation level",
  },
  target_hit: {
    label: "Target hit",
    cls: "border-blue-400/60 bg-blue-500/15 text-blue-200",
    title: "Live price has reached at least one author target",
  },
  unknown: {
    label: "No levels",
    cls: "border-slate-500/30 bg-slate-500/10 text-slate-300",
    title: "Note has no structured invalidation/targets — see body",
  },
}

function formatPrice(p: number | null | undefined): string {
  if (p == null) return "—"
  return `$${p.toFixed(2)}`
}

function formatPct(p: number | null | undefined, signed = true): string {
  if (p == null || !Number.isFinite(p)) return "—"
  const v = p * 100
  if (!signed) return `${v.toFixed(1)}%`
  const sign = v > 0 ? "+" : ""
  return `${sign}${v.toFixed(1)}%`
}

function ageColor(iso: string): string {
  const t = new Date(iso).getTime()
  if (Number.isNaN(t)) return "text-muted-foreground"
  const days = (Date.now() - t) / (1000 * 60 * 60 * 24)
  if (days >= 30) return "text-rose-300"
  if (days >= 7) return "text-amber-300"
  return "text-muted-foreground"
}

function quoteErrorLabel(err: QuoteError | null): string | null {
  if (!err) return null
  switch (err) {
    case "disconnected":
      return "TWS disconnected"
    case "no_permission":
      return "no market-data permission"
    case "timeout":
      return "quote timed out"
    case "fetch_failed":
      return "quote fetch failed"
  }
}

export function NoteValidityCard({ note }: NoteValidityCardProps) {
  const { quote, error } = useQuote(note.symbol)
  const live = quote?.lastPrice ?? null
  const result = classify(live, note)
  const pill = STATUS_PILL[result.status]
  const written = note.price_at_write
  const drift = live != null && written != null && written !== 0 ? (live - written) / written : null

  const targets = note.targets ?? []
  const hasLevels = note.invalidation_price != null || targets.length > 0

  return (
    <div className="border-border/60 bg-background/40 mt-2 rounded-md border p-2.5 text-xs">
      <div className="flex flex-wrap items-center gap-2">
        <span
          className={
            "rounded-full border px-2 py-0.5 text-[10px] tracking-wide uppercase " + pill.cls
          }
          title={pill.title}
        >
          {pill.label}
        </span>
        <span className="text-foreground font-mono">
          live {formatPrice(live)}
          {error && (
            <span className="ml-1 font-sans text-amber-300/80 normal-case">
              ({quoteErrorLabel(error)})
            </span>
          )}
        </span>
        {written != null && (
          <span className="text-muted-foreground font-mono">
            · written {formatPrice(written)}
            {drift != null && (
              <span
                className={
                  "ml-1 " + (drift > 0 ? "text-green-400" : drift < 0 ? "text-rose-300" : "")
                }
              >
                {formatPct(drift)}
              </span>
            )}
          </span>
        )}
        <span className={"ml-auto " + ageColor(note.written_at)} title={note.written_at}>
          {relativeTime(note.written_at)}
        </span>
      </div>

      {hasLevels && (
        <div className="text-muted-foreground mt-2 flex flex-wrap items-center gap-x-3 gap-y-1 font-mono">
          {note.invalidation_price != null && (
            <span>
              <span className="text-foreground/70">inv</span> {formatPrice(note.invalidation_price)}
              {result.buffer != null && (
                <span
                  className={
                    "ml-1 " +
                    (result.status === "invalidated"
                      ? "text-rose-300"
                      : result.status === "near_invalidation"
                        ? "text-amber-300"
                        : "text-muted-foreground")
                  }
                >
                  ({result.status === "invalidated" ? "breach" : "buf"} {formatPct(result.buffer)})
                </span>
              )}
            </span>
          )}
          {targets.map((t) => {
            const hit = result.status === "target_hit" && result.hitTarget?.label === t.label
            const distance = live != null && live !== 0 ? (t.price - live) / live : null
            return (
              <span key={t.label} className={hit ? "text-blue-300" : ""}>
                <span className="text-foreground/70">{t.label}</span> {formatPrice(t.price)}
                {distance != null && (
                  <span className="text-muted-foreground ml-1">({formatPct(distance)})</span>
                )}
                {hit && <span className="ml-1">·hit</span>}
              </span>
            )
          })}
          {note.catalyst_date && (
            <span className="text-muted-foreground">
              <span className="text-foreground/70">catalyst</span> {note.catalyst_date}
            </span>
          )}
        </div>
      )}
    </div>
  )
}
