/**
 * Phase 3 — pure grouping + aggregation for the Today's Trades panel.
 *
 * Two-level grouping:
 *   1. Top by `symbol`.
 *   2. Within a symbol, option fills bucket by the canonical 5-tuple
 *      (`expiry`, `strike`, `right`, `multiplier`). Stocks never get a
 *      sub-key — a stock-only group's `optionKey` is `null` and a stock
 *      + option mix on the same symbol surfaces as two distinct groups
 *      so the user reads each at its own granularity.
 *
 * Per-group totals follow the project rule that a missing commission
 * (`undefined`) is the truth, not a stand-in for `0`. `fees` excludes
 * `undefined` legs and the group's `feesPending` flag is set whenever
 * any leg's commission has not yet been reported. `grossRealized`
 * treats `undefined` `realized_pnl` as `0` (opening legs have none).
 *
 * Inter-group order: descending by the group's most recent fill so the
 * latest activity reads at the top. Intra-group: ascending by
 * `exec_time` so the user reads the leg sequence in execution order.
 */

import type { ExecutionRow, OptionKey, TradeGroup, TradesSummary } from "./types"

function optionKeyOf(row: ExecutionRow): OptionKey | null {
  if (row.contract_type !== "OPT") return null
  if (
    row.expiry === undefined ||
    row.strike === undefined ||
    row.right === undefined ||
    row.multiplier === undefined
  ) {
    return null
  }
  return {
    expiry: row.expiry,
    strike: row.strike,
    right: row.right,
    multiplier: row.multiplier,
  }
}

function bucketKey(row: ExecutionRow): string {
  const k = optionKeyOf(row)
  if (k === null) return `STK::${row.symbol}`
  return `OPT::${row.symbol}::${k.expiry}::${k.strike}::${k.right}::${k.multiplier}`
}

export function groupExecutions(rows: ExecutionRow[]): TradeGroup[] {
  const buckets = new Map<string, ExecutionRow[]>()
  for (const row of rows) {
    const key = bucketKey(row)
    const existing = buckets.get(key)
    if (existing) {
      existing.push(row)
    } else {
      buckets.set(key, [row])
    }
  }

  const groups: TradeGroup[] = []
  for (const legs of buckets.values()) {
    legs.sort((a, b) => a.time.localeCompare(b.time))
    const head = legs[0]
    let grossRealized = 0
    let fees = 0
    let feesPending = false
    for (const leg of legs) {
      grossRealized += leg.realized_pnl ?? 0
      if (leg.commission === undefined) {
        feesPending = true
      } else {
        fees += leg.commission
      }
    }
    groups.push({
      symbol: head.symbol,
      optionKey: optionKeyOf(head),
      legs,
      grossRealized,
      fees,
      netPnL: grossRealized - fees,
      feesPending,
      lastTime: legs[legs.length - 1].time,
    })
  }

  groups.sort((a, b) => b.lastTime.localeCompare(a.lastTime))
  return groups
}

export function summariseGroups(groups: TradeGroup[]): TradesSummary {
  let fills = 0
  let grossRealized = 0
  let fees = 0
  let feesPending = false
  for (const g of groups) {
    fills += g.legs.length
    grossRealized += g.grossRealized
    fees += g.fees
    if (g.feesPending) feesPending = true
  }
  return {
    fills,
    grossRealized,
    fees,
    netPnL: grossRealized - fees,
    feesPending,
  }
}
