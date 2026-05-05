import { describe, it, expect, vi, beforeEach } from "vitest"
import { render, screen, waitFor } from "@testing-library/react"

import { TodaysPlaybook } from "../components/TodaysPlaybook"
import type { Playbook } from "../types"

vi.mock("../../../shared/api/assessments", () => ({
  assessmentsApi: {
    getTradeReview: vi.fn(),
    getPlaybook: vi.fn(),
    getTraderProfile: vi.fn(),
  },
}))

import { assessmentsApi } from "../../../shared/api/assessments"

const playbook: Playbook = {
  date: "2026-05-05",
  account: "U1",
  generation_id: 2,
  generated_at: "2026-05-05T11:00:00Z",
  ranked_setups: [
    {
      symbol: "AAPL",
      bias: "long",
      trigger: "Reclaim 195",
      entry: "above 195.20",
      invalidation: "lose 194",
      target_1: "197",
      target_2: "199",
      conviction: "A",
      rationale_md: "Strong post-earnings drift",
      evidence_refs: [{ source: "news", note: "earnings beat" }],
    },
  ],
  skip_list: [{ symbol: "TSLA", reason: "recent chase_own_exit pattern" }],
  llm_call_id: null,
}

describe("TodaysPlaybook", () => {
  beforeEach(() => {
    vi.mocked(assessmentsApi.getPlaybook).mockReset()
  })

  it("renders empty-state when no playbook exists for the date", async () => {
    vi.mocked(assessmentsApi.getPlaybook).mockResolvedValue(null)
    render(<TodaysPlaybook date="2026-05-05" />)
    expect(await screen.findByText(/No playbook for 2026-05-05 yet/i)).toBeInTheDocument()
  })

  it("renders ranked setups + skip list when populated", async () => {
    vi.mocked(assessmentsApi.getPlaybook).mockResolvedValue(playbook)
    render(<TodaysPlaybook date="2026-05-05" />)

    await waitFor(() => {
      expect(screen.getByTestId("ranked-setups")).toBeInTheDocument()
    })
    const setups = screen.getByTestId("ranked-setups")
    expect(setups.textContent).toContain("AAPL")
    expect(setups.textContent).toContain("Reclaim 195")
    expect(setups.textContent).toContain("lose 194")

    const skip = screen.getByTestId("skip-list")
    expect(skip.textContent).toContain("TSLA")
    expect(skip.textContent).toContain("chase_own_exit")
  })

  it("surfaces backend errors", async () => {
    vi.mocked(assessmentsApi.getPlaybook).mockRejectedValue(new Error("boom"))
    render(<TodaysPlaybook date="2026-05-05" />)
    expect(await screen.findByText(/Failed to load playbook: boom/)).toBeInTheDocument()
  })
})
