import { useState } from "react"

import { Button } from "../../../shared/components/ui/button"
import { Card, CardContent } from "../../../shared/components/ui/card"

export interface EmptyTradeReviewProps {
  date: string
  onGenerate: () => Promise<void>
}

export function EmptyTradeReview({ date, onGenerate }: EmptyTradeReviewProps) {
  const [generating, setGenerating] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const handleClick = async () => {
    setGenerating(true)
    setError(null)
    try {
      await onGenerate()
    } catch (e) {
      setError(typeof e === "string" ? e : (e as Error).message)
    } finally {
      setGenerating(false)
    }
  }

  return (
    <Card>
      <CardContent className="text-muted-foreground space-y-3 py-12 text-center text-sm">
        <p>No trade review for {date} yet.</p>
        <p className="text-muted-foreground/70 text-xs">
          Reviews aren't written automatically — generate one now to pull the day's fills, FIFO-match
          them, and burn an LLM call for the narrative.
        </p>
        <div className="flex justify-center">
          <Button size="sm" onClick={() => void handleClick()} disabled={generating}>
            {generating ? "Generating…" : "Generate review"}
          </Button>
        </div>
        {error && (
          <p className="text-destructive text-xs" role="alert">
            {error}
          </p>
        )}
      </CardContent>
    </Card>
  )
}
