import { describe, it, expect, vi, beforeEach } from "vitest"
import { render, screen, waitFor } from "@testing-library/react"

import { TradeReviewCard } from "../components/TradeReviewCard"
import type { TradeReview } from "../types"

vi.mock("../../../shared/api/assessments", () => ({
  assessmentsApi: {
    getTradeReview: vi.fn(),
    getPlaybook: vi.fn(),
    getTraderProfile: vi.fn(),
  },
}))

import { assessmentsApi } from "../../../shared/api/assessments"

const review: TradeReview = {
  date: "2026-05-04",
  account: "U1",
  prompt_version: 1,
  generated_at: "2026-05-04T21:00:00Z",
  grade: "B",
  grade_score: 12.4,
  summary: {
    gross_pnl: 401.1,
    net_pnl: 380.0,
    commissions_total: 21.1,
    n_round_trips: 3,
    n_carryover: 0,
    win_rate: 0.667,
    by_symbol: { TSLA: 380.0 },
  },
  behavioral_tags: ["flat_close", "discipline_on_loser", "chase_own_exit"],
  leg_observations: [
    {
      leg_id: "leg-1",
      symbol: "TSLA",
      observation_md: "Sized small, exited on weakness",
      tag: "discipline_on_loser",
    },
  ],
  narrative_md: "Net positive day driven by TSLA scalps.",
  llm_call_id: null,
}

describe("TradeReviewCard", () => {
  beforeEach(() => {
    vi.mocked(assessmentsApi.getTradeReview).mockReset()
  })

  it("renders empty-state when no review exists for the date", async () => {
    vi.mocked(assessmentsApi.getTradeReview).mockResolvedValue(null)
    render(<TradeReviewCard date="2026-05-04" />)
    expect(await screen.findByText(/No trade review for 2026-05-04 yet/i)).toBeInTheDocument()
  })

  it("renders the grade, P&L summary, tags, and narrative when populated", async () => {
    vi.mocked(assessmentsApi.getTradeReview).mockResolvedValue(review)
    render(<TradeReviewCard date="2026-05-04" />)

    await waitFor(() => {
      expect(screen.getByTestId("grade-badge")).toBeInTheDocument()
    })
    const badge = screen.getByTestId("grade-badge")
    expect(badge.textContent).toContain("B")
    expect(badge.textContent).toContain("+12.4")

    const summary = screen.getByTestId("review-summary")
    expect(summary.textContent).toContain("$380.00")
    expect(summary.textContent).toContain("$401.10")
    expect(summary.textContent).toContain("$21.10")
    expect(summary.textContent).toContain("67%")

    const tags = screen.getByTestId("review-tags")
    expect(tags.textContent).toContain("flat_close")
    expect(tags.textContent).toContain("chase_own_exit")
    expect(tags.textContent).toContain("discipline_on_loser")

    expect(screen.getByText(/Net positive day driven by TSLA scalps/)).toBeInTheDocument()
  })

  it("surfaces backend errors", async () => {
    vi.mocked(assessmentsApi.getTradeReview).mockRejectedValue(new Error("boom"))
    render(<TradeReviewCard date="2026-05-04" />)
    expect(await screen.findByText(/Failed to load review: boom/)).toBeInTheDocument()
  })
})
