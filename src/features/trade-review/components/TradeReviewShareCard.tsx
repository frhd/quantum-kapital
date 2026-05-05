import { useRef, useState } from "react"
import { Check, Download } from "lucide-react"
import * as htmlToImage from "html-to-image"

import { shareApi } from "../../../shared/api/share"
import { Button } from "../../../shared/components/ui/button"
import { formatHumanDate, formatUsd, firstSentence, pnlColor } from "../lib/shareCardHelpers"
import type { TradeReview } from "../types"
import { BehavioralTagChip } from "./BehavioralTagChip"
import { GradeBadge } from "./GradeBadge"

const SIZE_PX = 420
const RESET_MS = 1500

type Status = "idle" | "saved" | "error"

export interface TradeReviewShareCardProps {
  review: TradeReview
  date: string
}

export function TradeReviewShareCard({ review, date }: TradeReviewShareCardProps) {
  const cardRef = useRef<HTMLDivElement>(null)
  const [status, setStatus] = useState<Status>("idle")
  const [errorMsg, setErrorMsg] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  const handleSave = async () => {
    if (!cardRef.current) return
    setBusy(true)
    setErrorMsg(null)
    let blob: Blob | null
    try {
      blob = await htmlToImage.toBlob(cardRef.current, { pixelRatio: 2, cacheBust: true })
    } catch (e) {
      setStatus("error")
      setErrorMsg(`Render failed: ${(e as Error).message}`)
      setBusy(false)
      return
    }
    if (!blob) {
      setStatus("error")
      setErrorMsg("Render produced no image data.")
      setBusy(false)
      return
    }
    try {
      const bytes = new Uint8Array(await blob.arrayBuffer())
      const savedPath = await shareApi.saveShareImagePng(date, bytes)
      if (savedPath !== null) {
        setStatus("saved")
        window.setTimeout(() => setStatus("idle"), RESET_MS)
      }
    } catch (e) {
      setStatus("error")
      setErrorMsg(`Save failed: ${typeof e === "string" ? e : (e as Error).message}`)
    } finally {
      setBusy(false)
    }
  }

  const dateLabel = formatHumanDate(date)
  const net = review.summary.net_pnl
  const winRate =
    review.summary.win_rate !== null && review.summary.win_rate !== undefined
      ? `${Math.round(review.summary.win_rate * 100)}% win rate`
      : null
  const trips = `${review.summary.n_round_trips} round trip${review.summary.n_round_trips === 1 ? "" : "s"}`
  const stats = [trips, winRate].filter((x): x is string => Boolean(x)).join(" · ")
  const takeaway = firstSentence(review.narrative_md)

  return (
    <div className="flex flex-col items-start gap-2">
      <div
        ref={cardRef}
        data-testid="share-card"
        style={{
          width: SIZE_PX,
          height: SIZE_PX,
          backgroundColor: "#0b0b0d",
          color: "#e4e4e7",
          fontFamily:
            'ui-sans-serif, system-ui, -apple-system, "Segoe UI", Roboto, "Helvetica Neue", Arial',
          border: "1px solid #27272a",
          borderRadius: 16,
          padding: 24,
          display: "flex",
          flexDirection: "column",
          justifyContent: "space-between",
        }}
      >
        <div style={{ display: "flex", justifyContent: "space-between", alignItems: "flex-start" }}>
          <div>
            <div
              style={{
                fontSize: 11,
                letterSpacing: 1,
                color: "#71717a",
                textTransform: "uppercase",
              }}
            >
              Trade Review
            </div>
            <div style={{ fontSize: 14, marginTop: 4, color: "#a1a1aa" }}>{dateLabel}</div>
          </div>
          {review.grade && review.grade_score != null && (
            <GradeBadge grade={review.grade} score={review.grade_score} />
          )}
        </div>

        <div style={{ textAlign: "center" }}>
          <div
            style={{
              fontSize: 44,
              fontWeight: 700,
              fontFamily:
                'ui-monospace, SFMono-Regular, "SF Mono", Menlo, Monaco, "Liberation Mono", monospace',
              color: pnlColor(net),
              fontVariantNumeric: "tabular-nums",
            }}
          >
            {formatUsd(net)}
          </div>
          {stats && <div style={{ fontSize: 13, color: "#a1a1aa", marginTop: 6 }}>{stats}</div>}
        </div>

        {takeaway && (
          <div
            style={{
              fontSize: 13,
              fontStyle: "italic",
              color: "#d4d4d8",
              textAlign: "center",
              lineHeight: 1.5,
              padding: "0 8px",
            }}
          >
            “{takeaway}”
          </div>
        )}

        <div>
          {review.behavioral_tags.length > 0 && (
            <div style={{ display: "flex", flexWrap: "wrap", gap: 6, justifyContent: "center" }}>
              {review.behavioral_tags.map((tag) => (
                <BehavioralTagChip key={tag} tag={tag} />
              ))}
            </div>
          )}
          <div
            style={{
              fontSize: 10,
              color: "#52525b",
              textAlign: "right",
              marginTop: 12,
              letterSpacing: 0.5,
            }}
          >
            quantum-kapital
          </div>
        </div>
      </div>

      <div className="flex items-center gap-2">
        <Button
          size="sm"
          variant="outline"
          onClick={() => void handleSave()}
          disabled={busy}
          className="h-8 px-3 text-xs"
        >
          {status === "saved" ? (
            <>
              <Check className="h-3.5 w-3.5" />
              <span>Saved</span>
            </>
          ) : (
            <>
              <Download className="h-3.5 w-3.5" />
              <span>{busy ? "Saving…" : "Save as PNG"}</span>
            </>
          )}
        </Button>
        {errorMsg && (
          <span className="text-destructive text-xs" role="alert">
            {errorMsg}
          </span>
        )}
      </div>
    </div>
  )
}
