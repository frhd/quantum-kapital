# Phase 7 — Frontend wrapper

> Part of [In-app trade-review generator](master.md). See master for invariants.

**Status:** done (commit daf8310, 2026-05-05)

**Depends on:** Phase 6 (Tauri command exists and is registered).

**Goal:** Add `assessmentsApi.generateTradeReview` so React components can trigger generation. The wrapper is the only place that names the command string; per `src/CLAUDE.md`, components must not call `invoke()` directly.

## Files

**Modify:**
- `src/shared/api/assessments.ts` — add `GenerateTradeReviewOpts` + `generateTradeReview` method.
- `src/shared/api/__tests__/assessments.test.ts` (create if it doesn't exist) — vitest covering payload shape + null result.

## Files to read before editing

- `src/shared/api/assessments.ts` — current shape; the new method goes inside the same `assessmentsApi` object.
- `src/features/trade-review/types.ts` — `TradeReview` type (re-used as the return type).
- Any existing `__tests__/*.test.ts` under `src/shared/api/` for the project's vitest mocking pattern (if none, the new test is allowed to define the pattern).

## Steps

- [ ] **Step 1: Check whether `src/shared/api/__tests__/` exists.**

```bash
ls src/shared/api/__tests__/ 2>/dev/null
```

If the directory exists, mirror an existing test's mocking style. If not, create it.

- [ ] **Step 2: Write the failing test.**

Create `src/shared/api/__tests__/assessments.test.ts`:

```ts
import { describe, expect, it, vi, beforeEach } from "vitest"
import type { TradeReview } from "../../../features/trade-review/types"

const invokeMock = vi.fn()
vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
}))

import { assessmentsApi } from "../assessments"

const fakeReview: TradeReview = {
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
    by_symbol: { AAPL: 99 },
  },
  behavioral_tags: ["flat_close"],
  leg_observations: [],
  narrative_md: "good day",
  llm_call_id: null,
}

describe("assessmentsApi.generateTradeReview", () => {
  beforeEach(() => invokeMock.mockReset())

  it("invokes generate_trade_review with the date and null account by default", async () => {
    invokeMock.mockResolvedValueOnce(fakeReview)
    const r = await assessmentsApi.generateTradeReview("2026-05-04")
    expect(invokeMock).toHaveBeenCalledWith("generate_trade_review", {
      date: "2026-05-04",
      account: null,
    })
    expect(r?.account).toBe("U1")
  })

  it("forwards an explicit account override", async () => {
    invokeMock.mockResolvedValueOnce(fakeReview)
    await assessmentsApi.generateTradeReview("2026-05-04", { account: "U999" })
    expect(invokeMock).toHaveBeenCalledWith("generate_trade_review", {
      date: "2026-05-04",
      account: "U999",
    })
  })

  it("returns null when the backend returns null (no-fills empty day)", async () => {
    invokeMock.mockResolvedValueOnce(null)
    const r = await assessmentsApi.generateTradeReview("2026-05-04")
    expect(r).toBeNull()
  })
})
```

- [ ] **Step 3: Run the failing test.**

Run: `pnpm test:run shared/api/__tests__/assessments.test.ts`
Expected: 3 tests fail (no `generateTradeReview` exported).

- [ ] **Step 4: Implement the wrapper.**

Edit `src/shared/api/assessments.ts`. Add to the existing options interfaces:

```ts
export interface GenerateTradeReviewOpts {
  account?: string | null
}
```

Add to the `assessmentsApi` object (just below `getTradeReview`):

```ts
  /** Generate a fresh trade review for `date` (ET, `YYYY-MM-DD`) by
   *  pulling the day's fills, FIFO-matching, asking the LLM to pick
   *  behavioral tags + write a narrative, and persisting via the
   *  TradeReviewStore. Idempotent — re-running for the same date
   *  overwrites the existing row.
   *
   *  Returns the populated review, or `null` if no fills exist for
   *  the day (the backend treats "no fills" as a non-error empty
   *  result so the UI can render a distinct state). */
  generateTradeReview: async (
    date: string,
    opts: GenerateTradeReviewOpts = {},
  ): Promise<TradeReview | null> => {
    return invoke<TradeReview | null>("generate_trade_review", {
      date,
      account: opts.account ?? null,
    })
  },
```

- [ ] **Step 5: Run the tests to confirm green.**

Run: `pnpm test:run shared/api/__tests__/assessments.test.ts`
Expected: 3 tests pass.

- [ ] **Step 6: Type-check + lint.**

Run: `pnpm typecheck && pnpm lint`
Expected: clean.

- [ ] **Step 7: Commit.**

```bash
git add src/shared/api/assessments.ts src/shared/api/__tests__/assessments.test.ts
git commit -m "$(cat <<'EOF'
feat(fe): assessmentsApi.generateTradeReview wrapper

Names the new generate_trade_review Tauri command in the only place
the FE is allowed to: shared/api. Returns Promise<TradeReview | null>
— null indicates an empty day (no fills) so callers can render a
distinct UI state.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```
