import { invoke } from "@tauri-apps/api/core"

// Phase 7 — exit-policy + bracket-reviser surface. Mirrors
// `strategies::exits::ExitPlan` + `services::bracket_reviser`.

export type PolicyVersion = "v1_static" | "v2_atr_scaled"

export interface ExitTargetSpec {
  label: string
  price: number
  qty_pct: number
  r_multiple: number | null
  atr_multiple: number | null
}

export interface TrailSpec {
  kind: "chandelier"
  atr_multiple: number
  activate_after_label: string | null
  move_to_break_even_at_r: number | null
}

export interface TimeStopSpec {
  max_trading_days: number
}

export interface ExitPlan {
  policy_version: PolicyVersion
  targets: ExitTargetSpec[]
  trail: TrailSpec | null
  time_stop: TimeStopSpec | null
  atr_at_signal: number | null
}

export interface ExitPolicyPreview {
  policy_version: PolicyVersion
  plan: ExitPlan | null
  error: string | null
}

export interface ChandelierState {
  extreme_price: number
  current_stop_price: number
  activated: boolean
  be_moved: boolean
  /** UTC ISO 8601, or null if no modify has fired. */
  last_modify_at: string | null
}

export interface BracketReviserSnapshot {
  parent_order_id: number
  setup_id: number
  symbol: string
  /** "long" | "short". */
  direction: string
  status: "open" | "partial" | "filled" | "stopped" | "canceled"
  stop_price: number
  trail_state: ChandelierState | null
  time_stop_remaining_days: number | null
}

export async function exitsGetPolicy(args: {
  strategy: string
  direction: "long" | "short"
  triggerPrice: number
  stopPrice: number
  atr: number | null
}): Promise<ExitPolicyPreview> {
  return await invoke("exits_get_policy", {
    strategy: args.strategy,
    direction: args.direction,
    triggerPrice: args.triggerPrice,
    stopPrice: args.stopPrice,
    atr: args.atr,
  })
}

/** Phase 7 stub — returns an error in the current build. The
 *  per-strategy override knob lands once the 4-week shadow run
 *  surfaces the comparator data; the wrapper exists so callers can
 *  see the same shape that will eventually succeed. */
export async function exitsSetPolicy(strategy: string, version: PolicyVersion): Promise<void> {
  return await invoke("exits_set_policy", { strategy, version })
}

export async function bracketReviserStatus(): Promise<BracketReviserSnapshot[]> {
  return await invoke("bracket_reviser_status")
}

export async function bracketRevertToStatic(parentOrderId: number) {
  return await invoke("bracket_revert_to_static", { parentOrderId })
}
