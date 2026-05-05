# Phase 8 — Empty-state button + accurate copy

> Part of [In-app trade-review generator](master.md). See master for invariants.

**Status:** todo

**Depends on:** Phase 7 (FE wrapper exists).

**Goal:** Replace the stale empty-state message with accurate copy + a "Generate review" button that wires through `assessmentsApi.generateTradeReview` and refreshes the card on success.

## Files

**Modify:**
- `src/features/trade-review/components/EmptyTradeReview.tsx` — add the button + accurate copy + loading/error states.
- `src/features/trade-review/components/TradeReviewCard.tsx` — pass an `onGenerate` callback into `EmptyTradeReview` (the card already owns the `refresh` machinery via `useTradeReview`).
- `src/features/trade-review/__tests__/TradeReviewCard.test.tsx` — extend with two new test cases (button visible in empty state; clicking the button calls the wrapper and refreshes).

## Why this exists

Today's copy is misleading on two counts:
- Says "Reviews are written automatically at 17:00 ET" — but the cron is just a template (`agent/cron/eod_review.cron`), not actually installed.
- References `uv run qk-eod-review --date <date>` — superseded by the `/eod-review` slash command and now also by this in-app button.

End state: the empty state's primary action is "Generate review now" (in-app); the copy describes the three real flows (in-app button, `/eod-review` slash command, optional cron) without lying about an auto-writer.

## Steps

- [ ] **Step 1: Write the failing card-level tests.**

Open `src/features/trade-review/__tests__/TradeReviewCard.test.tsx`. Inside the existing `describe("TradeReviewCard", ...)` block, add:

```tsx
import userEvent from "@testing-library/user-event"

it("shows a 'Generate review' button in the empty state", async () => {
  vi.mocked(assessmentsApi.getTradeReview).mockResolvedValue(null)
  render(<TradeReviewCard date="2026-05-04" />)
  const button = await screen.findByRole("button", { name: /generate review/i })
  expect(button).toBeInTheDocument()
})

it("clicking 'Generate review' calls the wrapper and refreshes the card on success", async () => {
  vi.mocked(assessmentsApi.getTradeReview).mockResolvedValue(null)
  vi.mocked(assessmentsApi.generateTradeReview).mockResolvedValue({
    date: "2026-05-04",
    account: "U1",
    prompt_version: 1,
    generated_at: "2026-05-04T22:00:00Z",
    grade: "B",
    grade_score: 5,
    summary: {
      gross_pnl: 100,
      net_pnl: 99,
      commissions_total: 1,
      n_round_trips: 1,
      n_carryover: 0,
      win_rate: 1,
      by_symbol: {},
    },
    behavioral_tags: [],
    leg_observations: [],
    narrative_md: "fresh narrative",
    llm_call_id: null,
  })

  render(<TradeReviewCard date="2026-05-04" />)
  const button = await screen.findByRole("button", { name: /generate review/i })

  // After click, the card should re-fetch via getTradeReview and show
  // the populated narrative.
  vi.mocked(assessmentsApi.getTradeReview).mockResolvedValueOnce({
    date: "2026-05-04",
    account: "U1",
    prompt_version: 1,
    generated_at: "2026-05-04T22:00:00Z",
    grade: "B",
    grade_score: 5,
    summary: {
      gross_pnl: 100,
      net_pnl: 99,
      commissions_total: 1,
      n_round_trips: 1,
      n_carryover: 0,
      win_rate: 1,
      by_symbol: {},
    },
    behavioral_tags: [],
    leg_observations: [],
    narrative_md: "fresh narrative",
    llm_call_id: null,
  })

  await userEvent.click(button)

  expect(assessmentsApi.generateTradeReview).toHaveBeenCalledWith("2026-05-04", {
    account: null,
  })
  await waitFor(() => {
    expect(screen.queryByText(/no trade review/i)).not.toBeInTheDocument()
  })
})

it("renders the typed error from generate_trade_review when the call fails", async () => {
  vi.mocked(assessmentsApi.getTradeReview).mockResolvedValue(null)
  vi.mocked(assessmentsApi.generateTradeReview).mockRejectedValueOnce(
    "daily budget exhausted",
  )
  render(<TradeReviewCard date="2026-05-04" />)
  const button = await screen.findByRole("button", { name: /generate review/i })
  await userEvent.click(button)
  expect(await screen.findByText(/daily budget exhausted/i)).toBeInTheDocument()
})
```

Make sure `assessmentsApi.generateTradeReview` is included in the existing top-of-file `vi.mocked(...)` reset block, e.g.:

```tsx
beforeEach(() => {
  vi.mocked(assessmentsApi.getTradeReview).mockReset()
  vi.mocked(assessmentsApi.generateTradeReview).mockReset()
})
```

- [ ] **Step 2: Run the failing tests.**

Run: `pnpm test:run features/trade-review/__tests__/TradeReviewCard.test.tsx`
Expected: the 3 new tests fail (no button rendered, no `onGenerate` plumbing).

- [ ] **Step 3: Replace `EmptyTradeReview.tsx`.**

```tsx
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
          Reviews aren't written automatically — generate one now, or run the{" "}
          <code className="bg-muted rounded px-1 py-0.5 font-mono text-xs">/eod-review</code>{" "}
          slash command from another Claude Code session.
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
```

- [ ] **Step 4: Wire the callback through `TradeReviewCard.tsx`.**

In `TradeReviewCard.tsx`, around the `useTradeReview` call, add the generate handler. Replace the `<EmptyTradeReview date={date} />` line with:

```tsx
<EmptyTradeReview
  date={date}
  onGenerate={async () => {
    await assessmentsApi.generateTradeReview(date, { account: account ?? null })
    await refresh()
  }}
/>
```

Add the import at the top:

```tsx
import { assessmentsApi } from "../../../shared/api/assessments"
```

- [ ] **Step 5: Run the tests to confirm green.**

Run: `pnpm test:run features/trade-review/__tests__/TradeReviewCard.test.tsx`
Expected: existing tests + the 3 new ones all pass.

- [ ] **Step 6: Type-check + lint + format.**

Run: `pnpm typecheck && pnpm lint && pnpm format`
Expected: clean.

- [ ] **Step 7: Manual smoke (no IBKR fills required).**

Pick a recent trading day with fills you remember (e.g. yesterday). Then:

```bash
pnpm tauri dev
# In the app: open Trade Review → DatePicker → 2026-05-04
# Click "Generate review"
# Watch /tmp/qk-tauri.log:
#   - tracing line for the LlmService call (kind=review)
#   - day_reviews UPSERT
# UI: card flips from empty state → populated review with grade + narrative.
```

If `executions(account, date)` returns empty, the wrapper resolves to `null` and the empty state stays put with no error toast — that's correct.

- [ ] **Step 8: Commit.**

```bash
git add src/features/trade-review/components/EmptyTradeReview.tsx \
        src/features/trade-review/components/TradeReviewCard.tsx \
        src/features/trade-review/__tests__/TradeReviewCard.test.tsx
git commit -m "$(cat <<'EOF'
feat(ui): generate-review button in trade-review empty state

Replaces the stale "auto-written at 17:00 ET" copy and obsolete
qk-eod-review CLI reference with a "Generate review" button that
invokes the new in-app generator, plus accurate copy describing the
three real flows (in-app button, /eod-review slash command, optional
cron). On success, refresh()'s the card so the populated row swaps in
without a manual refresh click.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

## Out-of-scope follow-ups (post-merge)

- **Thread `llm_call_id` through.** `TradeReviewGenerator` currently writes `llm_call_id: None`. Surface the inserted `llm_calls.id` from `LlmService::message` and pass it into `WriteTradeReviewRequest` so reviews are joinable to their generation call for cost/auditing.
- **Reuse the rust prompt for `/eod-review`.** Today the slash command's prompt lives in `agent/prompts/trade_review.md` — and the new Rust path now also `include_str!`s it. If the file ever drifts, both paths drift together. Lock the file under one path; symlink or duplicate-with-mirror-test, your call.
- **Pack-ideas section.** v1 omits today's playbook ranked ideas from the prompt; the Python path includes them. Add a `PlaybookStore::read_latest(date - 1)` lookup in the orchestrator and re-inject the section so behavioural-context parity is restored.
