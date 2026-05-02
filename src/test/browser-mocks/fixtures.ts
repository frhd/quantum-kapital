import type { DataTier } from "../../shared/types"
import {
  ACCT,
  accountSummary,
  agentMorningPack,
  alerts,
  calibrationStats,
  candidates,
  costAttribution,
  mcpAudit,
  morningPack,
  positions,
  predictionHistory,
  quote,
  researchNotes,
  settings,
  status,
  trackedTickers,
} from "./data"

type Fixture = (args: Record<string, unknown>) => unknown

export const fixtures: Record<string, Fixture> = {
  // Connection
  ibkr_get_connection_status: () => status(),
  ibkr_get_accounts: () => [ACCT],
  ibkr_get_data_tier: (): DataTier => "delayed",
  ibkr_connect: () => undefined,
  ibkr_disconnect: () => undefined,
  ibkr_start_daily_pnl: () => undefined,
  ibkr_stop_daily_pnl: () => undefined,
  ibkr_subscribe_market_data: () => undefined,

  // Portfolio
  ibkr_get_account_summary: () => accountSummary,
  ibkr_get_positions: () => positions,
  ibkr_get_quote: () => quote,
  ibkr_get_cached_tickers: () => ["NVDA", "TSLA", "MSFT"],

  // Settings
  get_settings: () => settings,
  get_settings_path: () => "/home/user/.config/quantum-kapital/settings.toml",

  // Tracker
  tracker_list: () => trackedTickers,
  tracker_get: ({ symbol }) => trackedTickers.find((t) => t.symbol === symbol) ?? null,
  tracker_list_alerts: () => alerts,
  tracker_mark_alerts_seen: ({ ids }) => (ids as number[]).length,
  tracker_get_morning_pack: () => morningPack,
  tracker_fetch_bars: () => [],
  tracker_get_news: () => [],

  // Candidates
  candidates_list: () => candidates,
  candidates_refresh_now: () => ({
    surge_upserted: 0,
    surge_auto_promoted: 0,
    decay_evicted: 0,
  }),

  // Research
  research_list_notes: () => researchNotes,
  research_get_note: ({ id }) => researchNotes.find((n) => n.id === id) ?? null,
  research_get_agent_morning_pack: () => agentMorningPack,
  research_list_agent_morning_packs: () => [agentMorningPack],
  research_list_mcp_audit: () => mcpAudit,

  // Eval
  eval_calibration_stats: () => calibrationStats,
  eval_cost_attribution: () => costAttribution,
  eval_prediction_history: () => predictionHistory,

  // Social sentiment
  social_get_latest: () => [],
  social_list_window: () => [],
  social_refresh_now: () => 0,
}
