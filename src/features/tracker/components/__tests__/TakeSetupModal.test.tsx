import { describe, it, expect, vi, beforeEach } from "vitest"
import { fireEvent, render, screen, waitFor } from "@testing-library/react"

import { TakeSetupModal } from "../TakeSetupModal"
import type { Setup } from "../../types"
import type { Sizing } from "@/shared/api/riskEngine"
import type { TicketReceipt } from "@/shared/api/orderTicket"

vi.mock("@/shared/api/orderTicket", async () => {
  const actual = await vi.importActual<typeof import("@/shared/api/orderTicket")>(
    "@/shared/api/orderTicket",
  )
  return {
    ...actual,
    orderTicketTakeSetup: vi.fn(),
  }
})

import { orderTicketTakeSetup } from "@/shared/api/orderTicket"

const SIZING: Sizing = {
  qty: 100,
  dollar_risk_cents: 25000,
  r_per_share_cents: 250,
  equity_at_decision_cents: 5_000_000,
  conviction_grade: "A",
  conviction_multiplier_bps: 10000,
  cap_applied: false,
  skipped_reason: null,
  version: 1,
}

function makeSetup(overrides: Partial<Setup> = {}): Setup {
  return {
    id: 42,
    symbol: "AAPL",
    strategy: "breakout",
    direction: "long",
    detected_at: "2026-05-05T13:00:00Z",
    trigger_price: 100,
    stop_price: 97.5,
    targets: [],
    raw_signals: null,
    thesis: null,
    thesis_json: null,
    status: "active",
    invalidated_at: null,
    invalidation_reason: null,
    archived_at: null,
    sizing: SIZING,
    ...overrides,
  }
}

const FRESH_FETCHED_AT = new Date(Date.now() - 60 * 60 * 1000).toISOString()
const STALE_FETCHED_AT = new Date(Date.now() - 48 * 60 * 60 * 1000).toISOString()

describe("TakeSetupModal", () => {
  beforeEach(() => {
    vi.mocked(orderTicketTakeSetup).mockReset()
  })

  it("blocks Send when sizing is null and surfaces the ungated alert", () => {
    const setup = makeSetup({ sizing: null })
    render(
      <TakeSetupModal open setup={setup} equityFetchedAt={FRESH_FETCHED_AT} onClose={() => {}} />,
    )
    expect(screen.getByTestId("gate-ungated")).toBeInTheDocument()
    expect(screen.getByTestId("take-setup-send")).toBeDisabled()
    expect(screen.queryByTestId("bracket-summary")).not.toBeInTheDocument()
  })

  it("blocks Send when equity is stale (>24h)", () => {
    render(
      <TakeSetupModal
        open
        setup={makeSetup()}
        equityFetchedAt={STALE_FETCHED_AT}
        onClose={() => {}}
      />,
    )
    expect(screen.getByTestId("gate-stale")).toBeInTheDocument()
    expect(screen.getByTestId("take-setup-send")).toBeDisabled()
  })

  it("blocks Send when override qty is set without a reason", () => {
    render(
      <TakeSetupModal
        open
        setup={makeSetup()}
        equityFetchedAt={FRESH_FETCHED_AT}
        onClose={() => {}}
      />,
    )
    fireEvent.click(screen.getByText(/Override qty \/ stop/))
    fireEvent.change(screen.getByLabelText(/^Qty$/), { target: { value: "50" } })
    expect(screen.getByTestId("override-reason-error")).toBeInTheDocument()
    expect(screen.getByTestId("take-setup-send")).toBeDisabled()
  })

  it("renders 50/30/20 rungs at trigger + NR for a long setup", () => {
    // trigger=100, stop=97.5, R=2.5: rungs at 102.5 / 105 / 107.5
    render(
      <TakeSetupModal
        open
        setup={makeSetup()}
        equityFetchedAt={FRESH_FETCHED_AT}
        onClose={() => {}}
      />,
    )
    expect(screen.getByTestId("rung-1R")).toHaveTextContent("$102.50")
    expect(screen.getByTestId("rung-2R")).toHaveTextContent("$105.00")
    expect(screen.getByTestId("rung-runner")).toHaveTextContent("$107.50")
  })

  it("renders 50/30/20 rungs at trigger - NR for a short setup", () => {
    // trigger=50, stop=52, R=2: rungs at 48 / 46 / 44
    const short = makeSetup({ direction: "short", trigger_price: 50, stop_price: 52 })
    render(
      <TakeSetupModal open setup={short} equityFetchedAt={FRESH_FETCHED_AT} onClose={() => {}} />,
    )
    expect(screen.getByTestId("rung-1R")).toHaveTextContent("$48.00")
    expect(screen.getByTestId("rung-2R")).toHaveTextContent("$46.00")
    expect(screen.getByTestId("rung-runner")).toHaveTextContent("$44.00")
  })

  it("calls the wrapper with the right args on submit", async () => {
    const receipt: TicketReceipt = {
      parent_order_id: 7,
      stop_order_id: 8,
      target_order_ids: [9, 10, 11],
      intent_id: "intent-abc",
      setup_id: 42,
      placed_at: "2026-05-05T14:00:00Z",
    }
    vi.mocked(orderTicketTakeSetup).mockResolvedValue(receipt)
    const onSubmitted = vi.fn()
    render(
      <TakeSetupModal
        open
        setup={makeSetup()}
        equityFetchedAt={FRESH_FETCHED_AT}
        onClose={() => {}}
        onSubmitted={onSubmitted}
      />,
    )
    fireEvent.click(screen.getByText(/Override qty \/ stop/))
    fireEvent.change(screen.getByLabelText(/^Qty$/), { target: { value: "75" } })
    fireEvent.change(screen.getByLabelText(/^Reason/), {
      target: { value: "trim into resistance" },
    })
    fireEvent.click(screen.getByTestId("take-setup-send"))
    await waitFor(() => {
      expect(orderTicketTakeSetup).toHaveBeenCalledWith({
        setupId: 42,
        overrideQty: 75,
        overrideStop: null,
        overrideReason: "trim into resistance",
      })
    })
    expect(onSubmitted).toHaveBeenCalledWith(receipt)
  })
})
