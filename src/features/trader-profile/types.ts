/**
 * Phase 7 — Trader Profile feature types.
 *
 * Mirrors `src-tauri/src/services/trader_profile/types.rs`. Pure SQL
 * aggregate over `day_reviews`; no LLM, no IBKR.
 */

import type { BehavioralTag } from "../trade-review/types"

export interface TagFrequency {
  tag: BehavioralTag
  count: number
  pct_of_reviews: number
}

export interface PnlByTag {
  tag: BehavioralTag
  n_days: number
  net_pnl_total: number
  net_pnl_per_day_avg: number
}

export interface WindowSummary {
  n_reviews: number
  tag_counts: Record<string, number>
  net_pnl: number
  avg_grade_score: number
}

export interface Trendline {
  last_7d: WindowSummary
  prior_21d: WindowSummary
}

export interface RecentIncident {
  /** ISO date `YYYY-MM-DD`, ET trading day. */
  date: string
  symbol: string
  tag: BehavioralTag
  leg_observation: string
}

export interface TraderProfile {
  account: string
  window_days: number
  /** ISO date `YYYY-MM-DD`. */
  since_date: string
  n_reviews: number
  tag_frequencies: TagFrequency[]
  pnl_by_tag: PnlByTag[]
  trendline: Trendline
  recent_incidents: RecentIncident[]
}
