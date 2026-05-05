import { invoke } from "@tauri-apps/api/core"

// Mirrors `services::tca::types`. Money fields are integer cents to
// dodge f64 round-trip drift through SQLite. Strategy is `null`
// for unattributed bucket (out-of-band TWS fills, pre-P2 history).

export interface AttributionRow {
  /** Detector class, e.g. "breakout". `null` ↔ unattributed bucket. */
  strategy: string | null
  n_trades: number
  gross_pnl_cents: number
  net_pnl_cents: number
  avg_slippage_bps: number
  n_with_slippage: number
  realized_pnl_cents: number
}

export interface SlippageBucket {
  lower_bps: number
  /** Top bucket uses the JS-numeric ceiling sentinel. */
  upper_bps: number
  n: number
}

export interface SlippageDistributionRow {
  strategy: string | null
  liquidity_bucket: string
  buckets: SlippageBucket[]
}

export type IntentSide = "buy" | "sell"

export interface ManualIntentArgs {
  setup_id?: number | null
  symbol: string
  side: IntentSide
  qty: number
  intended_price: number
  account?: string | null
}

/** Per-strategy roll-up over `[date_from, date_to]` ET trading days. */
export async function tcaGetAttribution(
  dateFrom: string,
  dateTo: string,
  account?: string,
): Promise<AttributionRow[]> {
  return await invoke("tca_get_attribution", {
    dateFrom,
    dateTo,
    account: account ?? null,
  })
}

/** Slippage histogram, one row per strategy + liquidity bucket. */
export async function tcaGetSlippageDistribution(
  dateFrom: string,
  dateTo: string,
  account?: string,
): Promise<SlippageDistributionRow[]> {
  return await invoke("tca_get_slippage_distribution", {
    dateFrom,
    dateTo,
    account: account ?? null,
  })
}

/** Trader-initiated intent for an order placed outside our UI. */
export async function tcaRecordManualIntent(args: ManualIntentArgs): Promise<string> {
  return await invoke("tca_record_manual_intent", { args })
}

// --- helpers for rendering ---

export function formatPnl(cents: number): string {
  const dollars = cents / 100
  if (dollars >= 0) return `+$${dollars.toFixed(2)}`
  return `-$${Math.abs(dollars).toFixed(2)}`
}

export function formatSlippageBps(bps: number): string {
  if (!Number.isFinite(bps)) return "—"
  return `${bps.toFixed(0)} bps`
}

export function formatStrategy(strategy: string | null): string {
  return strategy ?? "unattributed"
}

/** Format an upper-bps bucket bound, replacing the i64::MAX sentinel
 *  (round-tripped through serde as a very large number) with `+`. */
export function formatBucketRange(b: SlippageBucket): string {
  // Serde rounds i64::MAX to ~9.2e18 in JS; anything past 1e10 is the
  // top bucket sentinel.
  if (b.upper_bps > 1e10) return `${b.lower_bps}+`
  return `${b.lower_bps}–${b.upper_bps}`
}
