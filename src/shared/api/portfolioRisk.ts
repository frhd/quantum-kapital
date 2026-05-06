import { invoke } from "@tauri-apps/api/core"

// Mirrors `services::portfolio_risk`. Money in integer cents.

export type GateSeverity = "pass" | "warn" | "block"

export type ConcentrationKind = "total_risk" | "single_name" | "single_sector" | "factor_concurrent"

export interface GateLimitBreach {
  kind: ConcentrationKind
  label: string
  current: number
  projected: number
  limit: number
  /** 0 pass / 1 warn / 2 block. */
  severity: number
}

export interface GateResult {
  severity: GateSeverity
  breaches: GateLimitBreach[]
}

export interface ConcentrationConfig {
  max_total_pct_nlv: number
  max_sector_pct_nlv: number
  max_name_pct_nlv: number
  max_factor_concurrent: number
  warn_threshold: number
}

export interface PositionFactors {
  momentum: string
  value: string
  size: string
}

export interface OpenPosition {
  symbol: string
  qty: number
  /** +1 long / -1 short. */
  direction: number
  avg_cost_cents: number
  stop_cents: number
  stop_estimated: boolean
  dollar_risk_cents: number
  sector: string
  factors: PositionFactors
}

export interface SectorBucket {
  label: string
  dollar_risk_cents: number
  position_count: number
}

export interface FactorBucket {
  label: string
  count: number
}

export interface PortfolioRisk {
  snapshot_id: number
  account: string
  /** UTC ISO 8601 timestamp. */
  at: string
  nlv_cents: number
  total_dollar_risk_cents: number
  open_positions: OpenPosition[]
  by_sector: SectorBucket[]
  by_factor: FactorBucket[]
}

export interface PortfolioSnapshotRow {
  id: number
  account: string
  at: string
  nlv_cents: number
  total_dollar_risk_cents: number
  open_position_count: number
  exposures_json: unknown
}

export interface ConcentrationCheckInput {
  symbol: string
  projected_dollar_risk_cents: number
  strategy: string
  momentum_bucket?: string | null
}

export interface OverrideInput {
  setup_id: number
  gate_kind: string
  reason: string
  actor?: string | null
}

export interface PortfolioRiskChangedPayload {
  snapshot_id: number
  account: string
  nlv_cents: number
  total_dollar_risk_cents: number
  open_position_count: number
}

export async function portfolioRiskSnapshot(): Promise<PortfolioRisk> {
  return await invoke("portfolio_risk_snapshot")
}

export async function portfolioRiskHistory(limit?: number): Promise<PortfolioSnapshotRow[]> {
  return await invoke("portfolio_risk_history", { limit })
}

export async function concentrationGetConfig(): Promise<ConcentrationConfig> {
  return await invoke("concentration_get_config")
}

export async function concentrationSetConfig(cfg: ConcentrationConfig): Promise<void> {
  return await invoke("concentration_set_config", { cfg })
}

export async function concentrationCheck(input: ConcentrationCheckInput): Promise<GateResult> {
  return await invoke("concentration_check", { input })
}

export async function concentrationRecordOverride(input: OverrideInput): Promise<number> {
  return await invoke("concentration_record_override", { input })
}

// ----- formatting helpers -----

export function formatCents(cents: number): string {
  const dollars = cents / 100
  if (Math.abs(dollars) >= 1_000_000) return `$${(dollars / 1000).toFixed(0)}k`
  if (Math.abs(dollars) >= 1_000) return `$${(dollars / 1000).toFixed(1)}k`
  return `$${dollars.toFixed(0)}`
}

export function formatPctNlv(numerator: number, nlv: number): string {
  if (nlv <= 0) return "—"
  return `${((numerator / nlv) * 100).toFixed(1)}%`
}

export const GATE_KIND_LABELS: Record<ConcentrationKind, string> = {
  total_risk: "Total open risk",
  single_name: "Single name",
  single_sector: "Sector",
  factor_concurrent: "Factor concurrent",
}

export const GATE_KIND_COLORS: Record<ConcentrationKind, string> = {
  total_risk: "text-amber-400",
  single_name: "text-fuchsia-400",
  single_sector: "text-cyan-400",
  factor_concurrent: "text-emerald-400",
}
