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

/** Phase 4 — risk metrics surfaced on a v2 day_review row and via
 *  the `trade_review_get_metrics` Tauri command. Matches the Rust
 *  `RiskMetrics` struct in `services/trade_reviews/risk_metrics.rs`.
 *  Each annualized field is `null` when N < 20 daily samples (the
 *  "insufficient history" gate).
 */
export interface RiskMetrics {
  sharpe: number | null
  sortino: number | null
  calmar: number | null
  /** `Number.POSITIVE_INFINITY` when there are no losses. */
  profit_factor: number
  expectancy_r: number
  /** Positive fraction (0.10 = 10%). */
  max_dd: number
  max_dd_duration: number
  win_rate: number | null
  avg_win_r: number | null
  avg_loss_r: number | null
  n_days: number
  n_trades: number
  risk_free_rate_annual: number
}

export interface EquityPoint {
  /** ISO `YYYY-MM-DD`. */
  date: string
  equity: number
  daily_pnl: number
}

export interface StrategyRollup {
  /** Detector class string, or `"unattributed"` for legs without a
   *  setup-id linkage. */
  strategy: string
  n_trades: number
  realized_pnl: number
  avg_r: number | null
  win_rate: number | null
  profit_factor: number
  sharpe_30d: number | null
}

/** A persisted trade-review row.
 *
 * Phase 4 split: pre-P4 rows carry `formula_version="v1"` plus the
 * legacy `(grade, grade_score)` tuple; new rows carry
 * `formula_version="v2"` plus `(score_v2, discipline_v2,
 * risk_metrics, equity_curve)`. Never sum `score_v2` and
 * `discipline_v2` for ranking — they are surfaced separately.
 */
export interface TradeReview {
  /** ISO date `YYYY-MM-DD`, ET trading day. */
  date: string
  account: string
  prompt_version: number
  /** UTC ISO 8601. */
  generated_at: string
  /** `"v1"` or `"v2"`; tells you which scoring fields to read. */
  formula_version: string
  /** Pre-P4 legacy. `null` for v2 rows. */
  grade?: Grade | null
  /** Pre-P4 legacy. `null` for v2 rows. */
  grade_score?: number | null
  /** V2: Σ(realized_R × conviction_weight). `null` for v1 rows. */
  score_v2?: number | null
  /** V2: Σ(tag_weights). Surfaced separately. `null` for v1 rows. */
  discipline_v2?: number | null
  /** V2: Sharpe / Sortino / Calmar / PF / expectancy / DD. */
  risk_metrics?: RiskMetrics | null
  /** V2: per-day equity series for this review's date range. */
  equity_curve?: EquityPoint[] | null
  summary: LegSummary
  behavioral_tags: BehavioralTag[]
  leg_observations: LegObservation[]
  narrative_md: string
  llm_call_id?: string | null
}
