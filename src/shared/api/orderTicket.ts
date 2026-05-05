import { invoke } from "@tauri-apps/api/core"

// Mirrors `services::order_ticket::types`. Money fields are integer
// cents to dodge f64 round-trip drift through SQLite. Phase 3 ships a
// fixed 50/30/20 ladder; the percentages and per-rung qty are persisted
// per bracket so audits can reconstruct the modal exactly.

export type BracketStatus = "open" | "partial" | "filled" | "stopped" | "canceled"

export interface TargetSpec {
  label: string
  price: number
  qty: number
  qty_pct: number
}

export interface TicketReceipt {
  parent_order_id: number
  stop_order_id: number
  target_order_ids: number[]
  intent_id: string
  setup_id: number
  /** UTC ISO 8601 timestamp. */
  placed_at: string
}

export interface BracketGroupRecord {
  parent_order_id: number
  setup_id: number
  intent_id: string
  account: string
  symbol: string
  /** "long" | "short". Mirrors `setups.direction`. */
  direction: string
  parent_qty: number
  system_qty: number
  qty_override_reason: string | null
  entry_limit_cents: number
  stop_order_id: number
  stop_price_cents: number
  target_order_ids: number[]
  targets: TargetSpec[]
  /** UTC ISO 8601 timestamp. */
  placed_at: string
  last_status: BracketStatus
  /** UTC ISO 8601 timestamp. */
  last_status_at: string
}

export interface OrderTicketTakeSetupArgs {
  setupId: number
  overrideQty?: number | null
  overrideStop?: number | null
  overrideReason?: string | null
}

/** Place the bracket (parent + stop + targets) for a confirmed setup.
 *  Single human-confirmed entry point — never call from a scheduler /
 *  detector / agent path. */
export async function orderTicketTakeSetup(args: OrderTicketTakeSetupArgs): Promise<TicketReceipt> {
  return await invoke("order_ticket_take_setup", {
    setupId: args.setupId,
    overrideQty: args.overrideQty ?? null,
    overrideStop: args.overrideStop ?? null,
    overrideReason: args.overrideReason ?? null,
  })
}

/** Read current bracket-group state. `null` when no bracket has been
 *  placed for that parent order id. */
export async function orderTicketStatus(parentOrderId: number): Promise<BracketGroupRecord | null> {
  return await invoke("order_ticket_status", { parentOrderId })
}

/** Cancel an open bracket group (parent + children). */
export async function orderTicketCancelBracket(parentOrderId: number): Promise<BracketGroupRecord> {
  return await invoke("order_ticket_cancel_bracket", { parentOrderId })
}

// --- helpers for rendering ---

export const BRACKET_STATUS_LABELS: Record<BracketStatus, string> = {
  open: "Open",
  partial: "Partial",
  filled: "Filled",
  stopped: "Stopped",
  canceled: "Canceled",
}

export function formatBracketStatusLabel(s: BracketStatus): string {
  return BRACKET_STATUS_LABELS[s]
}

/** Mirrors `MAX_EQUITY_STALENESS_HOURS` in
 *  `services::order_ticket::types`. The modal hard-blocks Send when
 *  the equity snapshot is older than this. */
export const MAX_EQUITY_STALENESS_HOURS = 24

/** Static 50 / 30 / 20 ladder shipped in P3; matches
 *  `STATIC_TARGET_LADDER_PCT` / `STATIC_TARGET_R_MULTIPLES` in the
 *  Rust types module. The modal renders rungs from these constants
 *  so it stays byte-identical with the placer. */
export const STATIC_TARGET_LADDER: ReadonlyArray<{
  label: string
  pct: number
  rMultiple: number
}> = [
  { label: "1R", pct: 50, rMultiple: 1 },
  { label: "2R", pct: 30, rMultiple: 2 },
  { label: "runner", pct: 20, rMultiple: 3 },
]
