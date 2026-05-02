/**
 * Phase 8 — eval-harness frontend types.
 *
 * Mirrors the Rust shapes returned by `eval_*` Tauri commands. Kept in
 * lock-step with `services/eval_harness/mod.rs`.
 */

import type { Conviction, EvidenceRef } from "../research/types"

export interface ConvictionBucket {
  /** "A" | "B" | "C" | "overall" | null (ungraded) */
  conviction: string | null
  total: number
  hit_target: number
  hit_entry: number
  hit_invalidation: number
  drifted: number
  no_movement: number
  skipped: number
  unparseable: number
  /** (hit_target + hit_entry) / scoreable; 0 when no scoreable data. */
  win_rate: number
  /** hit_target / scoreable; 0 when no scoreable data. */
  target_rate: number
}

export interface CalibrationStats {
  window_days: number
  since_unix: number
  buckets: ConvictionBucket[]
  overall: ConvictionBucket
}

export interface CostBucket {
  /** loop_name when set, else "kind:<llm_kind>". */
  bucket: string
  call_count: number
  cost_usd: number
}

export interface CostAttribution {
  window_days: number
  since_unix: number
  total_cost_usd: number
  total_calls: number
  buckets: CostBucket[]
  a_conviction_count: number
  /** null when no A-conviction predictions in window (Rust NaN → JSON null). */
  usd_per_a_conviction: number | null
}

export interface Prediction {
  id: number
  source: string
  symbol: string
  conviction: Conviction | null
  entry_zone: string | null
  invalidation: string | null
  target: string | null
  thesis_md: string | null
  morning_pack_id: string | null
  predicted_at: string
}

export interface OutcomeRow {
  id: number
  pack_date: string
  symbol: string
  outcome_class: string
  conviction: Conviction | null
  entry_zone_low: number | null
  entry_zone_high: number | null
  invalidation_lvl: number | null
  realized_high: number
  realized_low: number
  realized_close: number
  eval_window_days: number
  evaluated_at: string
  prediction_id: number | null
}

export interface PredictionWithOutcome {
  prediction: Prediction
  outcome: OutcomeRow | null
}

// Re-export so the eval feature is self-contained for callers who
// don't already import from research.
export type { Conviction, EvidenceRef }
