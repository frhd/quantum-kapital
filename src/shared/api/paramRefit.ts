import { invoke } from "@tauri-apps/api/core"

// Mirrors `services::param_refit`. Wire shapes follow the
// snake_case serde defaults; the `_json` fields stay `unknown`
// since their detector-specific shape changes per phase.

export type RefitStatus = "locked" | "held" | "skipped" | "errored"
export type RefitSource = "cron" | "manual" | "backfill"

export interface ParamVintage {
  vintage_id: string
  detector: string
  params_json: unknown
  objective_value: number
  oos_n_trades: number
  /** ISO YYYY-MM-DD */
  train_window_from: string
  train_window_to: string
  oos_window_from: string
  oos_window_to: string
  /** UTC ISO 8601. */
  locked_at: string
  /** UTC ISO 8601 or null for the active vintage. */
  superseded_at: string | null
  source: string
  attempted_configs_json: unknown
  notes: string | null
}

export interface DetectorRefitOutcome {
  detector: string
  status: RefitStatus
  new_vintage: ParamVintage | null
  best_objective: number | null
  baseline_objective: number | null
  n_attempted: number
  n_constraints_passed: number
  note: string
}

export interface RefitReport {
  /** UTC ISO 8601. */
  refit_at: string
  source: string
  outcomes: DetectorRefitOutcome[]
}

export interface RunNowInput {
  detector?: string
}

export interface LockManualInput {
  detector: string
  params_json: unknown
  objective_value: number
  oos_n_trades: number
  notes?: string
}

export async function paramRefitRunNow(input?: RunNowInput): Promise<RefitReport> {
  return await invoke("param_refit_run_now", { input: input ?? null })
}

export async function paramRefitHistory(detector: string, limit?: number): Promise<ParamVintage[]> {
  return await invoke("param_refit_history", { detector, limit })
}

export async function paramRefitGetActive(): Promise<ParamVintage[]> {
  return await invoke("param_refit_get_active")
}

export async function paramRefitLockManual(input: LockManualInput): Promise<ParamVintage> {
  return await invoke("param_refit_lock_manual", { input })
}

// ----- formatting helpers -----

export const STATUS_LABELS: Record<RefitStatus, string> = {
  locked: "Locked",
  held: "Held",
  skipped: "Skipped",
  errored: "Errored",
}

export const STATUS_COLORS: Record<RefitStatus, string> = {
  locked: "text-emerald-500",
  held: "text-amber-500",
  skipped: "text-slate-400",
  errored: "text-red-500",
}

export function describeVintageWindow(v: ParamVintage): string {
  return `train ${v.train_window_from} → ${v.train_window_to}, OOS ${v.oos_window_from} → ${v.oos_window_to}`
}

export function isActiveVintage(v: ParamVintage): boolean {
  return v.superseded_at === null
}
