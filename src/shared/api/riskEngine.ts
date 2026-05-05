import { invoke } from "@tauri-apps/api/core"

// Mirrors `services::risk_engine::types`. All money fields are
// integer cents to dodge f64 round-trip drift through SQLite.

export type ConvictionGrade = "A" | "B" | "C"

export type EquitySource = "ibkr_account_summary" | "stale_cache" | "manual"

export type SizingSkippedReason =
  | "zero_r"
  | "below_min_risk"
  | "stale_snapshot"
  | "tilt_paused"
  | "invalid_price"

export interface Sizing {
  qty: number
  dollar_risk_cents: number
  r_per_share_cents: number
  equity_at_decision_cents: number
  conviction_grade: ConvictionGrade
  conviction_multiplier_bps: number
  cap_applied: boolean
  skipped_reason: SizingSkippedReason | null
  version: number
}

export interface EquitySnapshot {
  account: string
  /** ET trading date as ISO `YYYY-MM-DD`. */
  as_of_date: string
  nlv_cents: number
  source: EquitySource
  /** UTC ISO 8601 timestamp. */
  fetched_at: string
}

export interface RiskConfig {
  risk_pct_a: number
  risk_pct_b: number
  risk_pct_c: number
  max_position_pct: number
  min_dollar_risk: number
  conviction_multiplier_cap: number
  round_lot: number
}

export interface SetupSizedPayload {
  setup_id: number
  symbol: string
  sizing: Sizing
}

/** Read the live `RiskConfig`. */
export async function riskGetConfig(): Promise<RiskConfig> {
  return await invoke("risk_get_config")
}

/** Persist + push a new `RiskConfig`. */
export async function riskSetConfig(cfg: RiskConfig): Promise<void> {
  return await invoke("risk_set_config", { cfg })
}

/** Re-run sizing for a stored setup; emits `SetupSized` so any
 *  subscribed card refreshes without a manual reload. */
export async function riskRecomputeSetup(setupId: number): Promise<Sizing> {
  return await invoke("risk_recompute_setup", { setupId })
}

/** Force-fetch equity from IBKR. */
export async function riskRefreshEquity(): Promise<EquitySnapshot> {
  return await invoke("risk_refresh_equity")
}

// --- helpers for rendering ---

export function formatDollarRisk(cents: number): string {
  return `$${(cents / 100).toFixed(2)}`
}

export function formatRPerShare(cents: number): string {
  return `$${(cents / 100).toFixed(2)}`
}

export function formatEquity(cents: number): string {
  const dollars = cents / 100
  if (dollars >= 1_000_000) return `$${(dollars / 1000).toFixed(0)}k`
  if (dollars >= 1_000) return `$${(dollars / 1000).toFixed(1)}k`
  return `$${dollars.toFixed(0)}`
}

export const SIZING_SKIPPED_LABELS: Record<SizingSkippedReason, string> = {
  zero_r: "Zero R",
  below_min_risk: "Below min risk",
  stale_snapshot: "Stale equity",
  tilt_paused: "Tilt paused",
  invalid_price: "Invalid price",
}
