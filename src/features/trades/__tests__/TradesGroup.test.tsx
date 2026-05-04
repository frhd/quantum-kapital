import { describe, it, expect } from "vitest"
import { render, screen, within } from "@testing-library/react"
import { TradesGroup } from "../components/TradesGroup"
import { groupExecutions } from "../groupExecutions"
import type { ExecutionRow } from "../types"

function opt(over: Partial<ExecutionRow>): ExecutionRow {
  return {
    exec_id: "opt-1",
    time: "2026-05-04T18:00:00Z",
    account: "DU123",
    symbol: "TSLA",
    contract_type: "OPT",
    expiry: "2026-05-04",
    strike: 390,
    right: "C",
    multiplier: "100",
    side: "sold",
    qty: 1,
    avg_price: 5,
    commission: 0.65,
    realized_pnl: 75,
    currency: "USD",
    commission_currency: "USD",
    order_id: 7,
    ...over,
  }
}

describe("TradesGroup", () => {
  it("renders the group header with the option label, leg count, and gross/fees/net totals", () => {
    const groups = groupExecutions([
      opt({ exec_id: "a", time: "2026-05-04T18:00:00Z", commission: 0.65, realized_pnl: 50 }),
      opt({ exec_id: "b", time: "2026-05-04T18:30:00Z", commission: 0.65, realized_pnl: 25 }),
      opt({ exec_id: "c", time: "2026-05-04T19:00:00Z", commission: 0.65, realized_pnl: -10 }),
    ])
    expect(groups).toHaveLength(1)

    render(<TradesGroup group={groups[0]} />)

    const header = screen.getByTestId("trades-group-header")
    expect(header.textContent).toContain("TSLA 2026-05-04 $390 C")
    expect(header.textContent).toContain("3 legs")

    expect(screen.getByTestId("trades-group-gross").textContent).toBe("$65.00")
    expect(screen.getByTestId("trades-group-fees").textContent).toBe("$1.95")
    expect(screen.getByTestId("trades-group-net").textContent).toBe("$63.05")

    const legs = screen.getAllByTestId("trades-leg")
    expect(legs).toHaveLength(3)

    // Realized P&L cell on the losing leg shows the red class.
    const losingLeg = legs[2]
    const losingPnL = within(losingLeg).getByTestId("trades-leg-realized")
    expect(losingPnL.textContent).toBe("-$10.00")
    expect(losingPnL.className).toContain("text-red-400")
  })

  it("renders the fees-pending badge and an em-dash for the unreported leg", () => {
    const groups = groupExecutions([
      opt({ exec_id: "a", commission: 0.65, realized_pnl: 50 }),
      opt({
        exec_id: "b",
        commission: undefined,
        realized_pnl: 25,
        time: "2026-05-04T18:30:00Z",
      }),
    ])
    render(<TradesGroup group={groups[0]} />)

    expect(screen.getByText("fees pending")).toBeInTheDocument()

    const legs = screen.getAllByTestId("trades-leg")
    const pendingLeg = within(legs[1]).getByTestId("trades-leg-commission")
    expect(pendingLeg.textContent).toBe("—")
  })
})
