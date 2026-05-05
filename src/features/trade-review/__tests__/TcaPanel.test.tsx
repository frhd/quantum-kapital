import { describe, it, expect, vi, beforeEach } from "vitest"
import { render, screen, waitFor } from "@testing-library/react"

import { TcaPanel } from "../components/TcaPanel"
import type { AttributionRow, SlippageDistributionRow } from "@/shared/api/tca"

vi.mock("@/shared/api/tca", async () => {
  const actual = await vi.importActual<typeof import("@/shared/api/tca")>("@/shared/api/tca")
  return {
    ...actual,
    tcaGetAttribution: vi.fn(),
    tcaGetSlippageDistribution: vi.fn(),
  }
})

import { tcaGetAttribution, tcaGetSlippageDistribution } from "@/shared/api/tca"

const ATTRIBUTION_ROWS: AttributionRow[] = [
  {
    strategy: "breakout",
    n_trades: 4,
    gross_pnl_cents: 25000,
    net_pnl_cents: 24500,
    avg_slippage_bps: 32,
    n_with_slippage: 4,
    realized_pnl_cents: 25000,
  },
  {
    strategy: null,
    n_trades: 1,
    gross_pnl_cents: -1000,
    net_pnl_cents: -1100,
    avg_slippage_bps: 0,
    n_with_slippage: 0,
    realized_pnl_cents: -1000,
  },
]

const DISTRIBUTION_ROWS: SlippageDistributionRow[] = [
  {
    strategy: "breakout",
    liquidity_bucket: "all",
    buckets: [
      { lower_bps: 0, upper_bps: 1, n: 1 },
      { lower_bps: 1, upper_bps: 5, n: 1 },
      { lower_bps: 25, upper_bps: 50, n: 1 },
      { lower_bps: 100, upper_bps: 9223372036854775807, n: 1 },
    ],
  },
]

describe("TcaPanel", () => {
  beforeEach(() => {
    vi.mocked(tcaGetAttribution).mockReset()
    vi.mocked(tcaGetSlippageDistribution).mockReset()
  })

  it("renders attribution and slippage distribution from fixture data", async () => {
    vi.mocked(tcaGetAttribution).mockResolvedValue(ATTRIBUTION_ROWS)
    vi.mocked(tcaGetSlippageDistribution).mockResolvedValue(DISTRIBUTION_ROWS)
    render(<TcaPanel dateFrom="2026-05-01" dateTo="2026-05-04" />)

    expect(await screen.findByText("Attribution by strategy")).toBeInTheDocument()
    // "breakout" appears in both the attribution table and the
    // slippage-histogram header — use getAllByText.
    expect(screen.getAllByText("breakout").length).toBeGreaterThan(0)
    expect(screen.getByText("unattributed")).toBeInTheDocument()
    // Net P&L formatting +$245.00 for breakout, -$11.00 for unattributed.
    expect(screen.getByText("+$245.00")).toBeInTheDocument()
    expect(screen.getByText("-$11.00")).toBeInTheDocument()
    expect(screen.getByText("Slippage distribution")).toBeInTheDocument()
    // The 100+ sentinel bucket renders without overflow notation.
    expect(screen.getByText("100+")).toBeInTheDocument()
  })

  it("renders empty-state when no fills in the window", async () => {
    vi.mocked(tcaGetAttribution).mockResolvedValue([])
    vi.mocked(tcaGetSlippageDistribution).mockResolvedValue([])
    render(<TcaPanel dateFrom="2026-05-01" dateTo="2026-05-04" />)
    expect(await screen.findByText(/No fills in the selected window\./i)).toBeInTheDocument()
  })

  it("surfaces backend errors", async () => {
    vi.mocked(tcaGetAttribution).mockRejectedValue(new Error("boom"))
    vi.mocked(tcaGetSlippageDistribution).mockResolvedValue([])
    render(<TcaPanel dateFrom="2026-05-01" dateTo="2026-05-04" />)
    await waitFor(() => {
      expect(screen.getByText(/TCA error: boom/i)).toBeInTheDocument()
    })
  })
})
