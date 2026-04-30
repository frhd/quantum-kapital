import { useCallback, useEffect, useRef, useState } from "react"
import { createPortal } from "react-dom"
import { Button } from "../../../shared/components/ui/button"
import { Input } from "../../../shared/components/ui/input"
import { Label } from "../../../shared/components/ui/label"
import { Alert, AlertDescription } from "../../../shared/components/ui/alert"
import { AlertCircle, X } from "lucide-react"
import { ibkrApi } from "../../../shared/api/ibkr"
import {
  BUILT_IN_TAGS,
  type AddToTrackerPrefill,
  type StrategyTag,
  type TrackedTicker,
} from "../types"

interface AddToTrackerDialogProps {
  open: boolean
  prefill: AddToTrackerPrefill | null
  onClose: () => void
  onAdded: (ticker: TrackedTicker) => void
}

export function AddToTrackerDialog({ open, prefill, onClose, onAdded }: AddToTrackerDialogProps) {
  const [symbol, setSymbol] = useState("")
  const [tags, setTags] = useState<StrategyTag[]>([])
  const [customTag, setCustomTag] = useState("")
  const [notes, setNotes] = useState("")
  const [submitting, setSubmitting] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const symbolRef = useRef<HTMLInputElement>(null)
  const previouslyFocusedRef = useRef<HTMLElement | null>(null)

  useEffect(() => {
    if (!open) return
    previouslyFocusedRef.current = document.activeElement as HTMLElement | null
    setSymbol(prefill?.symbol ?? "")
    setTags(prefill?.tags ?? [])
    setNotes(prefill?.notes ?? "")
    setCustomTag("")
    setError(null)
    setSubmitting(false)
    requestAnimationFrame(() => {
      symbolRef.current?.focus()
      symbolRef.current?.select()
    })
  }, [open, prefill])

  const handleClose = useCallback(() => {
    onClose()
    const prev = previouslyFocusedRef.current
    if (prev && typeof prev.focus === "function") {
      prev.focus()
    }
  }, [onClose])

  useEffect(() => {
    if (!open) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault()
        handleClose()
      }
    }
    document.addEventListener("keydown", onKey)
    return () => document.removeEventListener("keydown", onKey)
  }, [open, handleClose])

  const toggleTag = (tag: StrategyTag) => {
    setTags((prev) => (prev.includes(tag) ? prev.filter((t) => t !== tag) : [...prev, tag]))
  }

  const addCustomTag = () => {
    const t = customTag.trim()
    if (!t) return
    if (!tags.includes(t)) setTags((prev) => [...prev, t])
    setCustomTag("")
  }

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    if (!symbol.trim()) {
      setError("Symbol is required")
      return
    }
    setSubmitting(true)
    setError(null)
    try {
      const ticker = await ibkrApi.tracker.add({
        symbol: symbol.trim().toUpperCase(),
        source: prefill?.source ?? "manual",
        sourceMeta: prefill?.sourceMeta ?? null,
        tags,
        notes: notes.trim() || null,
      })
      onAdded(ticker)
      handleClose()
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err)
      setError(
        msg.includes("already tracked") ? `${symbol.trim().toUpperCase()} is already tracked` : msg,
      )
    } finally {
      setSubmitting(false)
    }
  }

  if (!open) return null

  const customTags = tags.filter((tag) => !BUILT_IN_TAGS.some((b) => b.value === tag))
  const lockedSource = prefill?.source && prefill.source !== "manual"

  return createPortal(
    <div
      role="dialog"
      aria-modal="true"
      aria-labelledby="add-to-tracker-title"
      className="fixed inset-0 z-50 flex items-center justify-center p-4"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) handleClose()
      }}
    >
      <div className="absolute inset-0 bg-black/60" />
      <form
        onSubmit={handleSubmit}
        className="border-border bg-background text-foreground relative w-full max-w-md space-y-4 rounded-lg border p-5 shadow-xl"
      >
        <div className="flex items-start justify-between">
          <div>
            <h2 id="add-to-tracker-title" className="text-lg font-semibold">
              Add to tracker
            </h2>
            <p className="text-muted-foreground text-xs">
              Source:{" "}
              <span className="text-foreground font-medium">{prefill?.source ?? "manual"}</span>
              {lockedSource ? " (from scanner)" : ""}
            </p>
          </div>
          <Button
            type="button"
            variant="ghost"
            size="sm"
            onClick={handleClose}
            className="h-8 w-8 p-0"
            aria-label="Close"
          >
            <X className="h-4 w-4" />
          </Button>
        </div>

        {error && (
          <Alert variant="destructive">
            <AlertCircle className="h-4 w-4" />
            <AlertDescription>{error}</AlertDescription>
          </Alert>
        )}

        <div className="space-y-2">
          <Label htmlFor="symbol">Symbol</Label>
          <Input
            id="symbol"
            ref={symbolRef}
            value={symbol}
            onChange={(e) => setSymbol(e.target.value.toUpperCase())}
            disabled={Boolean(prefill?.symbol) && Boolean(lockedSource)}
            placeholder="AAPL"
            className="bg-card uppercase"
            autoComplete="off"
          />
        </div>

        <div className="space-y-2">
          <Label>Strategy tags</Label>
          <div className="flex flex-wrap gap-1">
            {BUILT_IN_TAGS.map((opt) => {
              const active = tags.includes(opt.value)
              return (
                <button
                  key={opt.value}
                  type="button"
                  onClick={() => toggleTag(opt.value)}
                  className={
                    "rounded-full border px-3 py-1 text-xs transition-colors " +
                    (active
                      ? "border-blue-400 bg-blue-500/20 text-blue-100"
                      : "border-input bg-card text-foreground hover:bg-secondary")
                  }
                >
                  {opt.label}
                </button>
              )
            })}
            {customTags.map((tag) => (
              <button
                key={tag}
                type="button"
                onClick={() => toggleTag(tag)}
                className="flex items-center gap-1 rounded-full border border-blue-400 bg-blue-500/20 px-3 py-1 text-xs text-blue-100"
              >
                {tag}
                <X className="h-3 w-3" />
              </button>
            ))}
          </div>
          <div className="flex gap-1">
            <Input
              value={customTag}
              onChange={(e) => setCustomTag(e.target.value)}
              placeholder="Custom tag…"
              className="bg-card h-8 text-xs"
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault()
                  addCustomTag()
                }
              }}
            />
            <Button
              type="button"
              variant="outline"
              size="sm"
              className="h-8 px-3 text-xs"
              onClick={addCustomTag}
              disabled={!customTag.trim()}
            >
              + Custom
            </Button>
          </div>
        </div>

        <div className="space-y-2">
          <Label htmlFor="notes">Notes (optional)</Label>
          <textarea
            id="notes"
            value={notes}
            onChange={(e) => setNotes(e.target.value)}
            rows={3}
            className="border-border bg-card text-foreground placeholder:text-muted-foreground w-full rounded-md border px-3 py-2 text-sm focus:ring-2 focus:ring-blue-500 focus:outline-hidden"
            placeholder="Why are you tracking this?"
          />
        </div>

        <div className="flex justify-end gap-2 pt-2">
          <Button type="button" variant="outline" onClick={handleClose} disabled={submitting}>
            Cancel
          </Button>
          <Button type="submit" disabled={submitting || !symbol.trim()}>
            {submitting ? "Adding…" : "Add"}
          </Button>
        </div>
      </form>
    </div>,
    document.body,
  )
}
