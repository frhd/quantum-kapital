import { describe, it, expect, vi, beforeEach } from "vitest"
import { fireEvent, render, screen, waitFor } from "@testing-library/react"

import { TradeReviewCard } from "../components/TradeReviewCard"
import type { TradeReview } from "../types"

vi.mock("../../../shared/api/assessments", () => ({
  assessmentsApi: {
    getTradeReview: vi.fn(),
    generateTradeReview: vi.fn(),
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
  formula_version: "v1",
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
  narrative_md:
    "Net positive day driven by TSLA scalps. Disciplined exits, no chasing. Watch for cleaner setups tomorrow.",
  llm_call_id: null,
}

describe("TradeReviewCard", () => {
  beforeEach(() => {
    vi.mocked(assessmentsApi.getTradeReview).mockReset()
    vi.mocked(assessmentsApi.generateTradeReview).mockReset()
  })

  it("renders empty-state when no review exists for the date", async () => {
    vi.mocked(assessmentsApi.getTradeReview).mockResolvedValue(null)
    render(<TradeReviewCard date="2026-05-04" />)
    expect(await screen.findByText(/No trade review for 2026-05-04 yet/i)).toBeInTheDocument()
  })

  it("renders the grade, share card, and narrative when populated", async () => {
    vi.mocked(assessmentsApi.getTradeReview).mockResolvedValue(review)
    render(<TradeReviewCard date="2026-05-04" />)

    // GradeBadge appears in the card header and inside the share card.
    await waitFor(() => {
      expect(screen.getAllByTestId("grade-badge").length).toBeGreaterThan(0)
    })
    const headerBadge = screen.getAllByTestId("grade-badge")[0]
    expect(headerBadge.textContent).toContain("B")
    expect(headerBadge.textContent).toContain("+12.4")

    const card = screen.getByTestId("share-card")
    expect(card.textContent).toContain("$380.00")
    expect(card.textContent).toContain("3 round trips")
    expect(card.textContent).toContain("67% win rate")
    expect(card.textContent).toContain("flat_close")
    expect(card.textContent).toContain("chase_own_exit")
    expect(card.textContent).toContain("discipline_on_loser")

    // Full narrative still renders below the share card.
    expect(screen.getByText(/Disciplined exits, no chasing/)).toBeInTheDocument()
  })

  it("surfaces backend errors", async () => {
    vi.mocked(assessmentsApi.getTradeReview).mockRejectedValue(new Error("boom"))
    render(<TradeReviewCard date="2026-05-04" />)
    expect(await screen.findByText(/Failed to load review: boom/)).toBeInTheDocument()
  })

  it("shows a 'Generate review' button in the empty state", async () => {
    vi.mocked(assessmentsApi.getTradeReview).mockResolvedValue(null)
    render(<TradeReviewCard date="2026-05-04" />)
    const button = await screen.findByRole("button", { name: /generate review/i })
    expect(button).toBeInTheDocument()
  })

  it("clicking 'Generate review' calls the wrapper and refreshes the card on success", async () => {
    vi.mocked(assessmentsApi.getTradeReview).mockResolvedValue(null)
    vi.mocked(assessmentsApi.generateTradeReview).mockResolvedValue(review)

    render(<TradeReviewCard date="2026-05-04" />)
    const button = await screen.findByRole("button", { name: /generate review/i })

    // After click, the card should re-fetch via getTradeReview and show
    // the populated narrative.
    vi.mocked(assessmentsApi.getTradeReview).mockResolvedValueOnce(review)

    fireEvent.click(button)

    await waitFor(() => {
      expect(assessmentsApi.generateTradeReview).toHaveBeenCalledWith("2026-05-04", {
        account: null,
      })
    })
    await waitFor(() => {
      expect(screen.queryByText(/no trade review/i)).not.toBeInTheDocument()
    })
  })

  it("renders the typed error from generate_trade_review when the call fails", async () => {
    vi.mocked(assessmentsApi.getTradeReview).mockResolvedValue(null)
    vi.mocked(assessmentsApi.generateTradeReview).mockRejectedValueOnce("daily budget exhausted")
    render(<TradeReviewCard date="2026-05-04" />)
    const button = await screen.findByRole("button", { name: /generate review/i })
    fireEvent.click(button)
    expect(await screen.findByText(/daily budget exhausted/i)).toBeInTheDocument()
  })

  it("shows a 'no fills' message when generate_trade_review resolves to null", async () => {
    vi.mocked(assessmentsApi.getTradeReview).mockResolvedValue(null)
    vi.mocked(assessmentsApi.generateTradeReview).mockResolvedValueOnce(null)
    render(<TradeReviewCard date="2026-05-04" />)
    const button = await screen.findByRole("button", { name: /generate review/i })
    fireEvent.click(button)
    expect(await screen.findByText(/No fills found for 2026-05-04/i)).toBeInTheDocument()
  })

  it("shows a 'Regenerate' button only when a review is populated", async () => {
    vi.mocked(assessmentsApi.getTradeReview).mockResolvedValue(review)
    render(<TradeReviewCard date="2026-05-04" />)
    expect(await screen.findByRole("button", { name: /regenerate/i })).toBeInTheDocument()
  })

  it("does not show 'Regenerate' in the empty state", async () => {
    vi.mocked(assessmentsApi.getTradeReview).mockResolvedValue(null)
    render(<TradeReviewCard date="2026-05-04" />)
    await screen.findByRole("button", { name: /generate review/i })
    expect(screen.queryByRole("button", { name: /^regenerate$/i })).not.toBeInTheDocument()
  })

  it("clicking 'Regenerate' confirms then re-runs the generator and refreshes", async () => {
    vi.mocked(assessmentsApi.getTradeReview).mockResolvedValue(review)
    vi.mocked(assessmentsApi.generateTradeReview).mockResolvedValue(review)
    const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(true)

    render(<TradeReviewCard date="2026-05-04" />)
    const button = await screen.findByRole("button", { name: /regenerate/i })
    fireEvent.click(button)

    await waitFor(() => {
      expect(confirmSpy).toHaveBeenCalled()
    })
    await waitFor(() => {
      expect(assessmentsApi.generateTradeReview).toHaveBeenCalledWith("2026-05-04", {
        account: null,
      })
    })
    confirmSpy.mockRestore()
  })

  it("clicking 'Regenerate' and dismissing the confirm does NOT call the generator", async () => {
    vi.mocked(assessmentsApi.getTradeReview).mockResolvedValue(review)
    const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(false)

    render(<TradeReviewCard date="2026-05-04" />)
    const button = await screen.findByRole("button", { name: /regenerate/i })
    fireEvent.click(button)

    await waitFor(() => {
      expect(confirmSpy).toHaveBeenCalled()
    })
    expect(assessmentsApi.generateTradeReview).not.toHaveBeenCalled()
    confirmSpy.mockRestore()
  })

  it("surfaces a regenerate error without clobbering the existing review", async () => {
    vi.mocked(assessmentsApi.getTradeReview).mockResolvedValue(review)
    vi.mocked(assessmentsApi.generateTradeReview).mockRejectedValueOnce("daily budget exhausted")
    const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(true)

    render(<TradeReviewCard date="2026-05-04" />)
    const button = await screen.findByRole("button", { name: /regenerate/i })
    fireEvent.click(button)

    expect(await screen.findByText(/daily budget exhausted/i)).toBeInTheDocument()
    // Existing review still rendered below the error banner — assert via the
    // narrative body, which is unique to the populated content.
    expect(screen.getByText(/Disciplined exits, no chasing/)).toBeInTheDocument()
    confirmSpy.mockRestore()
  })
})
