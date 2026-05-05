import { describe, it, expect, vi, beforeEach } from "vitest"
import { render, screen, waitFor } from "@testing-library/react"

import { TraderProfilePage } from "../components/TraderProfilePage"
import type { TraderProfile } from "../types"

vi.mock("../../../shared/api/assessments", () => ({
  assessmentsApi: {
    getTradeReview: vi.fn(),
    getPlaybook: vi.fn(),
    getTraderProfile: vi.fn(),
  },
}))

import { assessmentsApi } from "../../../shared/api/assessments"

const emptyProfile: TraderProfile = {
  account: "U1",
  window_days: 30,
  since_date: "2026-04-05",
  n_reviews: 0,
  tag_frequencies: [],
  pnl_by_tag: [],
  trendline: {
    last_7d: { n_reviews: 0, tag_counts: {}, net_pnl: 0, avg_grade_score: 0 },
    prior_21d: { n_reviews: 0, tag_counts: {}, net_pnl: 0, avg_grade_score: 0 },
  },
  recent_incidents: [],
}

const populatedProfile: TraderProfile = {
  account: "U1",
  window_days: 30,
  since_date: "2026-04-05",
  n_reviews: 10,
  tag_frequencies: [
    { tag: "flat_close", count: 8, pct_of_reviews: 0.8 },
    { tag: "chase_own_exit", count: 3, pct_of_reviews: 0.3 },
  ],
  pnl_by_tag: [
    {
      tag: "flat_close",
      n_days: 8,
      net_pnl_total: 1200.0,
      net_pnl_per_day_avg: 150.0,
    },
    {
      tag: "chase_own_exit",
      n_days: 3,
      net_pnl_total: -450.0,
      net_pnl_per_day_avg: -150.0,
    },
  ],
  trendline: {
    last_7d: {
      n_reviews: 5,
      tag_counts: { flat_close: 4, chase_own_exit: 2 },
      net_pnl: 540.0,
      avg_grade_score: 11.5,
    },
    prior_21d: {
      n_reviews: 5,
      tag_counts: { flat_close: 4, chase_own_exit: 1 },
      net_pnl: 210.0,
      avg_grade_score: 8.0,
    },
  },
  recent_incidents: [
    {
      date: "2026-05-03",
      symbol: "TSLA",
      tag: "chase_own_exit",
      leg_observation: "Re-entered TSLA 2 minutes after take-profit.",
    },
  ],
}

describe("TraderProfilePage", () => {
  beforeEach(() => {
    vi.mocked(assessmentsApi.getTraderProfile).mockReset()
  })

  it("renders empty-state when profile has no reviews", async () => {
    vi.mocked(assessmentsApi.getTraderProfile).mockResolvedValue(emptyProfile)
    render(<TraderProfilePage />)
    expect(await screen.findByText(/Profile is empty/i)).toBeInTheDocument()
  })

  it("renders trendline, tag frequencies, P&L attribution, and incidents when populated", async () => {
    vi.mocked(assessmentsApi.getTraderProfile).mockResolvedValue(populatedProfile)
    render(<TraderProfilePage />)

    await waitFor(() => {
      expect(screen.getByTestId("trendline")).toBeInTheDocument()
    })
    expect(screen.getByTestId("tag-frequency-chart").textContent).toContain("flat_close")
    expect(screen.getByTestId("tag-frequency-chart").textContent).toContain("chase_own_exit")
    expect(screen.getByTestId("pnl-by-tag-heatmap").textContent).toContain("$1200.00")
    expect(screen.getByTestId("recent-incidents").textContent).toContain("TSLA")
    expect(screen.getByTestId("recent-incidents").textContent).toContain(
      "Re-entered TSLA 2 minutes after take-profit",
    )
  })

  it("surfaces backend errors", async () => {
    vi.mocked(assessmentsApi.getTraderProfile).mockRejectedValue(new Error("boom"))
    render(<TraderProfilePage />)
    expect(await screen.findByText(/Failed to load profile: boom/)).toBeInTheDocument()
  })
})
