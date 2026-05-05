/**
 * Phase 7 — Tauri command wrappers for the assessment stack.
 *
 * Mirrors the FE-facing Tauri commands in
 * `src-tauri/src/ibkr/commands/assessments.rs` (which themselves wrap
 * the same services as the `get_trade_review`, `get_today_playbook`,
 * and `get_trader_profile` MCP read tools). This is the only place
 * that names the command strings.
 */

import { invoke } from "@tauri-apps/api/core"

import type { TradeReview } from "../../features/trade-review/types"
import type { Playbook } from "../../features/playbook/types"
import type { TraderProfile } from "../../features/trader-profile/types"

export interface GetTradeReviewOpts {
  account?: string | null
  promptVersion?: number | null
}

export interface GetPlaybookOpts {
  account?: string | null
  generationId?: number | null
}

export interface GetTraderProfileOpts {
  windowDays?: number | null
  account?: string | null
}

export const assessmentsApi = {
  /** Returns the structured trade review for `date` (ET, `YYYY-MM-DD`),
   *  or `null` if no row was written. Defaults to the latest
   *  `prompt_version` for the date when omitted. */
  getTradeReview: async (
    date: string,
    opts: GetTradeReviewOpts = {},
  ): Promise<TradeReview | null> => {
    return invoke<TradeReview | null>("get_trade_review", {
      date,
      account: opts.account ?? null,
      promptVersion: opts.promptVersion ?? null,
    })
  },

  /** Returns the structured playbook for `date` (ET, `YYYY-MM-DD`), or
   *  `null` if no row was written. Defaults to the latest
   *  `generation_id` for the date when omitted. */
  getPlaybook: async (date: string, opts: GetPlaybookOpts = {}): Promise<Playbook | null> => {
    return invoke<Playbook | null>("get_today_playbook", {
      date,
      account: opts.account ?? null,
      generationId: opts.generationId ?? null,
    })
  },

  /** Returns the trader's behavioral profile aggregated over the
   *  trailing `windowDays` (default 30; clamped to [1, 365]). */
  getTraderProfile: async (opts: GetTraderProfileOpts = {}): Promise<TraderProfile> => {
    return invoke<TraderProfile>("get_trader_profile", {
      windowDays: opts.windowDays ?? null,
      account: opts.account ?? null,
    })
  },
}
