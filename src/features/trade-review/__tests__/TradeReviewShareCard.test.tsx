import { describe, it, expect, vi, beforeEach } from "vitest"
import { fireEvent, render, screen, waitFor } from "@testing-library/react"

import { TradeReviewShareCard } from "../components/TradeReviewShareCard"
import type { TradeReview } from "../types"

vi.mock("html-to-image", () => ({
  toBlob: vi.fn(),
}))
vi.mock("../../../shared/api/share", () => ({
  shareApi: {
    saveShareImagePng: vi.fn(),
  },
}))

import * as htmlToImage from "html-to-image"
import { shareApi } from "../../../shared/api/share"

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
  behavioral_tags: ["flat_close", "discipline_on_loser"],
  leg_observations: [],
  narrative_md: "Net positive day driven by TSLA scalps.",
  llm_call_id: null,
}

const fakeBlob = new Blob(["png"], { type: "image/png" })

describe("TradeReviewShareCard", () => {
  beforeEach(() => {
    vi.mocked(htmlToImage.toBlob).mockReset()
    vi.mocked(shareApi.saveShareImagePng).mockReset()
  })

  it("renders date, hero P&L, stats, takeaway, and tags", () => {
    render(<TradeReviewShareCard review={review} date="2026-05-04" />)
    const card = screen.getByTestId("share-card")
    expect(card.textContent).toContain("Mon May 4")
    expect(card.textContent).toContain("$380.00")
    expect(card.textContent).toContain("3 round trips")
    expect(card.textContent).toContain("67% win rate")
    expect(card.textContent).toContain("Net positive day driven by TSLA scalps.")
    expect(card.textContent).toContain("flat_close")
    expect(card.textContent).toContain("quantum-kapital")
  })

  it("renders negative P&L with a leading minus", () => {
    const r = { ...review, summary: { ...review.summary, net_pnl: -125.5 } }
    render(<TradeReviewShareCard review={r} date="2026-05-04" />)
    expect(screen.getByTestId("share-card").textContent).toContain("-$125.50")
  })

  it("uses singular 'round trip' when count is 1", () => {
    const r = { ...review, summary: { ...review.summary, n_round_trips: 1 } }
    render(<TradeReviewShareCard review={r} date="2026-05-04" />)
    const card = screen.getByTestId("share-card")
    expect(card.textContent).toContain("1 round trip ·")
    expect(card.textContent).not.toContain("1 round trips")
  })

  it("clicking 'Save as PNG' renders the card and hands the bytes to the save command", async () => {
    vi.mocked(htmlToImage.toBlob).mockResolvedValue(fakeBlob)
    vi.mocked(shareApi.saveShareImagePng).mockResolvedValue("/Users/x/trade-review-2026-05-04.png")

    render(<TradeReviewShareCard review={review} date="2026-05-04" />)
    fireEvent.click(screen.getByRole("button", { name: /save as png/i }))

    await waitFor(() => {
      expect(htmlToImage.toBlob).toHaveBeenCalledTimes(1)
    })
    const node = vi.mocked(htmlToImage.toBlob).mock.calls[0][0]
    expect(node).toBe(screen.getByTestId("share-card"))

    await waitFor(() => {
      expect(shareApi.saveShareImagePng).toHaveBeenCalledTimes(1)
    })
    const [calledDate, calledBytes] = vi.mocked(shareApi.saveShareImagePng).mock.calls[0]
    expect(calledDate).toBe("2026-05-04")
    expect(calledBytes).toBeInstanceOf(Uint8Array)

    expect(await screen.findByText(/^saved$/i)).toBeInTheDocument()
  })

  it("stays idle when the user cancels the save dialog", async () => {
    vi.mocked(htmlToImage.toBlob).mockResolvedValue(fakeBlob)
    vi.mocked(shareApi.saveShareImagePng).mockResolvedValue(null)

    render(<TradeReviewShareCard review={review} date="2026-05-04" />)
    fireEvent.click(screen.getByRole("button", { name: /save as png/i }))

    await waitFor(() => {
      expect(shareApi.saveShareImagePng).toHaveBeenCalled()
    })
    // Button label remains "Save as PNG" — no "Saved" feedback after cancel.
    expect(screen.queryByText(/^saved$/i)).not.toBeInTheDocument()
    expect(screen.getByRole("button", { name: /save as png/i })).toBeInTheDocument()
  })

  it("surfaces a render error when html-to-image throws", async () => {
    vi.mocked(htmlToImage.toBlob).mockRejectedValueOnce(new Error("boom"))
    render(<TradeReviewShareCard review={review} date="2026-05-04" />)
    fireEvent.click(screen.getByRole("button", { name: /save as png/i }))
    expect(await screen.findByRole("alert")).toHaveTextContent(/render failed: boom/i)
  })

  it("surfaces a save error when the Tauri command fails", async () => {
    vi.mocked(htmlToImage.toBlob).mockResolvedValue(fakeBlob)
    vi.mocked(shareApi.saveShareImagePng).mockRejectedValueOnce("disk full")
    render(<TradeReviewShareCard review={review} date="2026-05-04" />)
    fireEvent.click(screen.getByRole("button", { name: /save as png/i }))
    expect(await screen.findByRole("alert")).toHaveTextContent(/save failed: disk full/i)
  })
})
