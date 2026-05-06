import { invoke } from "@tauri-apps/api/core"

// Mirrors `services::regime`. The Rust enums use snake_case serde so
// the string literals here are the wire format.

export type TrendAxis = "up" | "sideways" | "down"
export type VolAxis = "low" | "normal" | "high"
export type BreadthAxis = "healthy" | "mixed" | "narrow"
export type CorrAxis = "low" | "mixed" | "high"

export interface Regime {
  trend: TrendAxis
  vol: VolAxis
  breadth: BreadthAxis
  corr: CorrAxis
}

export interface RegimeFilter {
  trend: TrendAxis[]
  vol: VolAxis[]
  breadth: BreadthAxis[]
  corr: CorrAxis[]
}

export type RegimeSnapshotSource = "daily_close" | "intraday" | "force_recompute"

export interface RegimeSnapshotRow {
  id: number
  /** UTC ISO 8601. */
  at: string
  raw: Regime
  stable: Regime
  inputs_json: unknown
  source: string
}

export interface RegimeCurrent {
  snapshot_id: number
  at_unix: number
  raw: Regime
  stable: Regime
  source: string
  inputs_summary: unknown
  missing: string[]
}

export interface RegimeConfig {
  enabled: boolean
  min_monthly_trades_floor: number
  per_detector: Record<string, RegimeFilter>
}

export interface RegimeOverrideInput {
  setup_id: number
  reason: string
  actor?: string | null
}

export interface RegimeChangedPayload {
  snapshot_id: number
  regime: Regime
  source: string
}

export async function regimeCurrent(): Promise<RegimeCurrent> {
  return await invoke("regime_current")
}

export async function regimeHistory(limit?: number): Promise<RegimeSnapshotRow[]> {
  return await invoke("regime_history", { limit })
}

export async function regimeForceRecompute(): Promise<RegimeCurrent> {
  return await invoke("regime_force_recompute")
}

export async function regimeGetConfig(): Promise<RegimeConfig> {
  return await invoke("regime_get_config")
}

export async function regimeSetConfig(cfg: RegimeConfig): Promise<void> {
  return await invoke("regime_set_config", { cfg })
}

export async function regimeRecordOverride(input: RegimeOverrideInput): Promise<number> {
  return await invoke("regime_record_override", { input })
}

// ----- formatting helpers -----

export const TREND_LABELS: Record<TrendAxis, string> = {
  up: "Up",
  sideways: "Sideways",
  down: "Down",
}

export const VOL_LABELS: Record<VolAxis, string> = {
  low: "Low",
  normal: "Normal",
  high: "High",
}

export const BREADTH_LABELS: Record<BreadthAxis, string> = {
  healthy: "Healthy",
  mixed: "Mixed",
  narrow: "Narrow",
}

export const CORR_LABELS: Record<CorrAxis, string> = {
  low: "Low",
  mixed: "Mixed",
  high: "High",
}

/** Compact one-line summary used in the SkippedSetupsPanel. */
export function describeRegime(regime: Regime): string {
  return `trend=${regime.trend} vol=${regime.vol} breadth=${regime.breadth} corr=${regime.corr}`
}

/** Human-readable summary of a per-detector filter. Mirrors
 *  `RegimeFilter::describe` on the Rust side. */
export function describeFilter(filter: RegimeFilter): string {
  const parts: string[] = []
  if (filter.trend.length) parts.push(`trend in [${filter.trend.join(",")}]`)
  if (filter.vol.length) parts.push(`vol in [${filter.vol.join(",")}]`)
  if (filter.breadth.length) parts.push(`breadth in [${filter.breadth.join(",")}]`)
  if (filter.corr.length) parts.push(`corr in [${filter.corr.join(",")}]`)
  return parts.length ? parts.join(" && ") : "any regime"
}
