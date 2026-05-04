/**
 * Phase 3 — Today's Trades panel.
 *
 * TypeScript mirror of the Rust `ExecutionRow` DTO in
 * `src-tauri/src/mcp/tools/executions.rs`. The MCP tool and the Tauri
 * command both serve this shape so the desktop UI and a Claude Code
 * agent see byte-identical rows.
 *
 * Manual mirror is intentional — the project does not run codegen
 * (per `src/CLAUDE.md`). Field renames here MUST match a backend
 * change in the same commit.
 */

export type ExecutionSide = "bought" | "sold"

export interface ExecutionRow {
  exec_id: string
  /** UTC ISO 8601 — convert to ET at the formatting site only. */
  time: string
  account: string
  symbol: string
  /** IBKR `secType`: `"STK"`, `"OPT"`, ... */
  contract_type: string
  /** Option fields are omitted (undefined) for non-option rows because
   *  the Rust DTO uses `skip_serializing_if = "Option::is_none"`. */
  expiry?: string
  strike?: number
  right?: string
  multiplier?: string
  side: ExecutionSide
  qty: number
  avg_price: number
  /** `undefined` ↔ "not (yet) reported by IBKR"; literal `0` is real. */
  commission?: number
  /** Realized P&L — closing legs only; gross of the closing leg's commission. */
  realized_pnl?: number
  currency?: string
  commission_currency?: string
  order_id: number
}

/** Composite key for an option contract — the canonical 5-tuple used
 *  to group spread legs together within a symbol. Stocks never get
 *  a sub-key. */
export interface OptionKey {
  expiry: string
  strike: number
  right: string
  multiplier: string
}

export interface TradeGroup {
  /** `symbol` for stock-only groups; for options the same symbol may
   *  produce multiple groups, one per strike. */
  symbol: string
  /** Set when every leg is an option fill on the same contract. */
  optionKey: OptionKey | null
  legs: ExecutionRow[]
  /** Sum of `realized_pnl`, treating `undefined` as 0 (opening legs
   *  carry no realized P&L). */
  grossRealized: number
  /** Sum of reported commissions only — `undefined` legs are excluded
   *  so missing commissions don't silently mis-attribute fees. */
  fees: number
  /** `grossRealized - fees`. */
  netPnL: number
  /** True when at least one leg has `commission === undefined`. */
  feesPending: boolean
  /** Latest `time` across the legs, used for inter-group ordering. */
  lastTime: string
}

export interface TradesSummary {
  fills: number
  grossRealized: number
  fees: number
  netPnL: number
  feesPending: boolean
}
