/**
 * Phase 4 (quant-decisions): Tauri command wrappers for the v2
 * trade-review analytics surface.
 *
 * Mirrors the FE-facing Tauri commands in
 * `src-tauri/src/ibkr/commands/trade_review_metrics.rs`. This is the
 * only place that names the command strings.
 */

import { invoke } from "@tauri-apps/api/core"

import type {
  EquityPoint,
  RiskMetrics,
  StrategyRollup,
} from "../../features/trade-review/types"

export interface DateRangeOpts {
  account?: string | null
}

export interface EquityCurveOpts extends DateRangeOpts {
  startingEquity?: number | null
}

export const tradeReviewMetricsApi = {
  /** Roll up Sharpe / Sortino / Calmar / profit-factor / expectancy /
   *  max-DD across `[start, end]` (inclusive, ET trading days). The
   *  Sharpe / Sortino / Calmar fields return `null` when fewer than
   *  20 daily samples are available — the UI must render
   *  "insufficient history" rather than the noisy short-window number.
   */
  getMetrics: async (
    start: string,
    end: string,
    opts: DateRangeOpts = {},
  ): Promise<RiskMetrics> => {
    return invoke<RiskMetrics>("trade_review_get_metrics", {
      start,
      end,
      account: opts.account ?? null,
    })
  },

  /** Daily equity series across `[start, end]`. `startingEquity`
   *  defaults to 0 (cumulative trade-flow equity since the range
   *  start). Pass T-1 NLV for an "intraday since 09:30" view. */
  getEquityCurve: async (
    start: string,
    end: string,
    opts: EquityCurveOpts = {},
  ): Promise<EquityPoint[]> => {
    return invoke<EquityPoint[]>("trade_review_get_equity_curve", {
      start,
      end,
      account: opts.account ?? null,
      startingEquity: opts.startingEquity ?? null,
    })
  },

  /** Per-strategy (= detector class) rollup across `[start, end]`.
   *  Legs whose opening fill carried no `strategy` land in the
   *  `"unattributed"` bucket. `avg_r` is `null` when no leg in the
   *  bucket could be reduced to an R (missing setup linkage or
   *  NULL dollar-risk on the linked setup). */
  getStrategyRollup: async (
    start: string,
    end: string,
    opts: DateRangeOpts = {},
  ): Promise<StrategyRollup[]> => {
    return invoke<StrategyRollup[]>("trade_review_get_strategy_rollup", {
      start,
      end,
      account: opts.account ?? null,
    })
  },
}
