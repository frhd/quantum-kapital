/**
 * Phase 7 — browser-mocks fixtures for the assessment surface
 * (`get_trade_review`, `get_today_playbook`, `get_trader_profile`).
 *
 * Kept separate from `data.ts` so the broader fixture file stays
 * under the file-size cap. Mirrors the same realism convention: just
 * enough plausible structure to render the panels under
 * `pnpm dev:browser`, not a backend simulator.
 */

import { ACCT, isoMinusDays, now } from "./data"
import type { TradeReview } from "../../features/trade-review/types"
import type { Playbook } from "../../features/playbook/types"
import type { TraderProfile } from "../../features/trader-profile/types"

const today = new Date().toISOString().slice(0, 10)

export const tradeReview: TradeReview = {
  date: today,
  account: ACCT,
  prompt_version: 1,
  generated_at: now(),
  formula_version: "v1",
  grade: "B",
  grade_score: 12.4,
  summary: {
    gross_pnl: 401.1,
    net_pnl: 380.0,
    commissions_total: 21.1,
    n_round_trips: 3,
    n_carryover: 0,
    win_rate: 0.667,
    by_symbol: { TSLA: 380.0 },
  },
  behavioral_tags: ["flat_close", "discipline_on_loser", "chase_own_exit"],
  leg_observations: [
    {
      leg_id: "leg-1",
      symbol: "TSLA",
      observation_md: "Sized small, exited on weakness — held discipline through the chop.",
      tag: "discipline_on_loser",
    },
  ],
  narrative_md:
    "Net positive day driven by TSLA scalps. Re-entered after take-profit once, otherwise discipline held.",
  llm_call_id: null,
}

export const playbook: Playbook = {
  date: today,
  account: ACCT,
  generation_id: 1,
  generated_at: now(),
  ranked_setups: [
    {
      symbol: "AAPL",
      bias: "long",
      trigger: "Reclaim 195",
      entry: "above 195.20",
      invalidation: "lose 194",
      target_1: "197",
      target_2: "199",
      conviction: "A",
      rationale_md: "Strong post-earnings drift; clean reclaim of yesterday's high.",
      evidence_refs: [{ source: "news", note: "earnings beat by $0.12" }],
    },
    {
      symbol: "NVDA",
      bias: "short",
      trigger: "Reject 510",
      entry: "below 508",
      invalidation: "above 511",
      target_1: "503",
      conviction: "B",
      rationale_md: "Daily lower-high in the making.",
      evidence_refs: [],
    },
  ],
  skip_list: [{ symbol: "TSLA", reason: "recent chase_own_exit pattern (3 of last 7 days)" }],
  llm_call_id: null,
}

export const traderProfile: TraderProfile = {
  account: ACCT,
  window_days: 30,
  since_date: isoMinusDays(30).slice(0, 10),
  n_reviews: 12,
  tag_frequencies: [
    { tag: "flat_close", count: 9, pct_of_reviews: 0.75 },
    { tag: "discipline_on_loser", count: 6, pct_of_reviews: 0.5 },
    { tag: "chase_own_exit", count: 4, pct_of_reviews: 0.33 },
    { tag: "off_thesis_trade", count: 2, pct_of_reviews: 0.17 },
  ],
  pnl_by_tag: [
    { tag: "flat_close", n_days: 9, net_pnl_total: 1820.0, net_pnl_per_day_avg: 202.22 },
    { tag: "discipline_on_loser", n_days: 6, net_pnl_total: 940.0, net_pnl_per_day_avg: 156.67 },
    { tag: "chase_own_exit", n_days: 4, net_pnl_total: -640.0, net_pnl_per_day_avg: -160.0 },
    { tag: "off_thesis_trade", n_days: 2, net_pnl_total: -180.0, net_pnl_per_day_avg: -90.0 },
  ],
  trendline: {
    last_7d: {
      n_reviews: 5,
      tag_counts: { flat_close: 4, chase_own_exit: 2, discipline_on_loser: 3 },
      net_pnl: 540.0,
      avg_grade_score: 11.5,
    },
    prior_21d: {
      n_reviews: 7,
      tag_counts: { flat_close: 5, chase_own_exit: 2, discipline_on_loser: 3 },
      net_pnl: 1100.0,
      avg_grade_score: 9.2,
    },
  },
  recent_incidents: [
    {
      date: isoMinusDays(2).slice(0, 10),
      symbol: "TSLA",
      tag: "chase_own_exit",
      leg_observation: "Re-entered TSLA 0DTE 4 minutes after take-profit.",
    },
    {
      date: isoMinusDays(5).slice(0, 10),
      symbol: "TSLA",
      tag: "chase_own_exit",
      leg_observation: "Reopened identical strike within 2 minutes of closing.",
    },
  ],
}
