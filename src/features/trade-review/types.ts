/**
 * Phase 7 — Trade Review feature types.
 *
 * TypeScript mirrors of the Rust `TradeReview` DTO and its components in
 * `src-tauri/src/services/trade_reviews/`. The MCP read tool and the
 * Tauri command both serve this shape. The closed `BehavioralTag` enum
 * is mirrored from the Rust source — drift is caught by the
 * `agent/tests/test_tag_mirror.py` mirror test plus a Python prompt
 * test, but the FE list still has to be hand-updated when the Rust
 * enum gains a value (same convention as `ExecutionRow`).
 */

export type Grade = "A" | "B" | "C" | "D" | "F"

export const BEHAVIORAL_TAGS = [
  "chase_own_exit",
  "late_otm_lottery",
  "gamma_window_violation",
  "single_name_concentration",
  "position_sizing_ungraduated",
  "post_loss_revenge",
  "flat_close",
  "discipline_on_loser",
  "scaled_in_winner",
  "scaled_in_loser",
  "thesis_match_executed",
  "off_thesis_trade",
] as const

export type BehavioralTag = (typeof BEHAVIORAL_TAGS)[number]

/** Per-tag weight mirrored from `services/trade_reviews/grade.rs::tag_weight`. */
export const TAG_WEIGHTS: Record<BehavioralTag, number> = {
  chase_own_exit: -10,
  late_otm_lottery: -10,
  gamma_window_violation: -5,
  single_name_concentration: -5,
  position_sizing_ungraduated: -5,
  post_loss_revenge: -5,
  flat_close: 5,
  discipline_on_loser: 5,
  scaled_in_winner: 3,
  scaled_in_loser: -7,
  thesis_match_executed: 5,
  off_thesis_trade: -3,
}

export interface LegObservation {
  leg_id: string
  symbol?: string
  observation_md: string
  tag?: BehavioralTag
}

export interface LegSummary {
  gross_pnl: number
  net_pnl: number
  commissions_total: number
  n_round_trips: number
  n_carryover: number
  win_rate?: number | null
  by_symbol: Record<string, number>
}

export interface TradeReview {
  /** ISO date `YYYY-MM-DD`, ET trading day. */
  date: string
  account: string
  prompt_version: number
  /** UTC ISO 8601. */
  generated_at: string
  grade: Grade
  grade_score: number
  summary: LegSummary
  behavioral_tags: BehavioralTag[]
  leg_observations: LegObservation[]
  narrative_md: string
  llm_call_id?: string | null
}
