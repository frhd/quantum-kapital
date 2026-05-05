import { describe, expect, it, vi, beforeEach } from "vitest"
import type { TradeReview } from "../../../features/trade-review/types"

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }))
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
