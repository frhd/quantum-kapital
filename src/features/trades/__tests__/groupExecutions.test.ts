import { describe, it, expect } from "vitest"
import { groupExecutions, summariseGroups } from "../groupExecutions"
import type { ExecutionRow } from "../types"

function stk(over: Partial<ExecutionRow>): ExecutionRow {
  return {
    exec_id: "stk-1",
    time: "2026-05-04T14:30:00Z",
    account: "DU123",
    symbol: "AAPL",
    contract_type: "STK",
    side: "bought",
    qty: 100,
    avg_price: 150,
    commission: 1,
    realized_pnl: undefined,
    currency: "USD",
    commission_currency: "USD",
    order_id: 1,
    ...over,
  }
}

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

describe("groupExecutions", () => {
  it("groups stock-only fills under a single symbol group with no option key", () => {
    const groups = groupExecutions([
      stk({ exec_id: "a", symbol: "AAPL" }),
      stk({ exec_id: "b", symbol: "AAPL", time: "2026-05-04T15:00:00Z" }),
      stk({ exec_id: "c", symbol: "MSFT" }),
    ])

    const aapl = groups.find((g) => g.symbol === "AAPL" && g.optionKey === null)
    const msft = groups.find((g) => g.symbol === "MSFT" && g.optionKey === null)
    expect(aapl).toBeDefined()
    expect(msft).toBeDefined()
    expect(aapl!.legs.map((l) => l.exec_id)).toEqual(["a", "b"])
    expect(msft!.legs).toHaveLength(1)
  })

  it("buckets option fills by the (expiry, strike, right, multiplier) tuple within a symbol", () => {
    const groups = groupExecutions([
      opt({ exec_id: "c1", strike: 390 }),
      opt({ exec_id: "c2", strike: 395 }),
      opt({ exec_id: "c3", strike: 390, time: "2026-05-04T18:30:00Z" }),
    ])

    expect(groups).toHaveLength(2)
    const k390 = groups.find((g) => g.optionKey?.strike === 390)
    const k395 = groups.find((g) => g.optionKey?.strike === 395)
    expect(k390!.legs.map((l) => l.exec_id).sort()).toEqual(["c1", "c3"])
    expect(k395!.legs.map((l) => l.exec_id)).toEqual(["c2"])
  })

  it("does not collapse a stock and an option for the same symbol into one group", () => {
    const groups = groupExecutions([
      stk({ exec_id: "tsla-stk", symbol: "TSLA" }),
      opt({ exec_id: "tsla-opt", symbol: "TSLA" }),
    ])
    expect(groups).toHaveLength(2)
    const stockGroup = groups.find((g) => g.optionKey === null && g.symbol === "TSLA")
    const optGroup = groups.find((g) => g.optionKey !== null && g.symbol === "TSLA")
    expect(stockGroup).toBeDefined()
    expect(optGroup).toBeDefined()
  })

  it("computes per-group gross/fees/net and orders legs ascending by time", () => {
    const groups = groupExecutions([
      opt({
        exec_id: "first",
        time: "2026-05-04T18:00:00Z",
        commission: 0.65,
        realized_pnl: 50,
      }),
      opt({
        exec_id: "second",
        time: "2026-05-04T18:30:00Z",
        commission: 0.65,
        realized_pnl: 25,
      }),
    ])
    expect(groups).toHaveLength(1)
    const g = groups[0]
    expect(g.legs.map((l) => l.exec_id)).toEqual(["first", "second"])
    expect(g.grossRealized).toBeCloseTo(75)
    expect(g.fees).toBeCloseTo(1.3)
    expect(g.netPnL).toBeCloseTo(73.7)
    expect(g.feesPending).toBe(false)
  })

  it("propagates commission=undefined into the group's feesPending flag and excludes it from fees", () => {
    const groups = groupExecutions([
      opt({ exec_id: "with-fee", commission: 0.65, realized_pnl: 50 }),
      opt({
        exec_id: "no-fee",
        commission: undefined,
        realized_pnl: 25,
        time: "2026-05-04T18:30:00Z",
      }),
    ])
    expect(groups).toHaveLength(1)
    const g = groups[0]
    expect(g.fees).toBeCloseTo(0.65)
    expect(g.grossRealized).toBeCloseTo(75)
    expect(g.netPnL).toBeCloseTo(74.35)
    expect(g.feesPending).toBe(true)
  })

  it("orders groups by the most recent fill (descending lastTime)", () => {
    const groups = groupExecutions([
      stk({ exec_id: "msft-old", symbol: "MSFT", time: "2026-05-04T14:00:00Z" }),
      stk({ exec_id: "aapl-mid", symbol: "AAPL", time: "2026-05-04T15:00:00Z" }),
      stk({ exec_id: "tsla-late", symbol: "TSLA", time: "2026-05-04T19:00:00Z" }),
    ])
    expect(groups.map((g) => g.symbol)).toEqual(["TSLA", "AAPL", "MSFT"])
  })

  it("returns an empty array for empty input", () => {
    expect(groupExecutions([])).toEqual([])
  })
})

describe("summariseGroups", () => {
  it("sums fills, gross, fees, and net across groups; OR's the feesPending flag", () => {
    const groups = groupExecutions([
      opt({ exec_id: "a", commission: 0.65, realized_pnl: 50 }),
      opt({
        exec_id: "b",
        commission: undefined,
        realized_pnl: 25,
        time: "2026-05-04T18:30:00Z",
      }),
      stk({ exec_id: "c", symbol: "AAPL", commission: 1.0 }),
    ])
    const summary = summariseGroups(groups)
    expect(summary.fills).toBe(3)
    expect(summary.grossRealized).toBeCloseTo(75)
    expect(summary.fees).toBeCloseTo(1.65)
    expect(summary.netPnL).toBeCloseTo(73.35)
    expect(summary.feesPending).toBe(true)
  })

  it("returns zeros for empty input", () => {
    expect(summariseGroups([])).toEqual({
      fills: 0,
      grossRealized: 0,
      fees: 0,
      netPnL: 0,
      feesPending: false,
    })
  })
})
