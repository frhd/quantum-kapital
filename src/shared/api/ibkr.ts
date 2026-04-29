import { invoke } from "@tauri-apps/api/core"
import type {
  ConnectionConfig,
  ConnectionStatus,
  AccountSummary,
  Position,
  OrderRequest,
  FundamentalData,
  ScenarioProjections,
  ProjectionResults,
  ProjectionAssumptions,
  ScannerSubscription,
} from "../types"
import type {
  TrackedTicker,
  TrackerSource,
  TrackerStatus,
  StrategyTag,
  NewsItem,
  HistoricalBar,
  BarSize,
} from "../../features/tracker/types"

export const ibkrApi = {
  connect: async (config: ConnectionConfig) => {
    return invoke("ibkr_connect", { config })
  },

  disconnect: async () => {
    return invoke("ibkr_disconnect")
  },

  getConnectionStatus: async () => {
    return invoke<ConnectionStatus>("ibkr_get_connection_status")
  },

  getAccounts: async () => {
    return invoke<string[]>("ibkr_get_accounts")
  },

  getAccountSummary: async (account: string) => {
    return invoke<AccountSummary[]>("ibkr_get_account_summary", { account })
  },

  getPositions: async () => {
    return invoke<Position[]>("ibkr_get_positions")
  },

  startDailyPnL: async (account: string) => {
    return invoke<void>("ibkr_start_daily_pnl", { account })
  },

  stopDailyPnL: async () => {
    return invoke<void>("ibkr_stop_daily_pnl")
  },

  subscribeMarketData: async (symbol: string) => {
    return invoke("ibkr_subscribe_market_data", { symbol })
  },

  placeOrder: async (order: OrderRequest) => {
    return invoke("ibkr_place_order", { order })
  },

  getFundamentalData: async (symbol: string) => {
    return invoke<FundamentalData>("ibkr_get_fundamental_data", { symbol })
  },

  generateProjections: async (symbol: string, assumptions?: ProjectionAssumptions) => {
    return invoke<ScenarioProjections>("ibkr_generate_projections", { symbol, assumptions })
  },

  generateProjectionResults: async (symbol: string, assumptions?: ProjectionAssumptions) => {
    return invoke<ProjectionResults>("ibkr_generate_projection_results", { symbol, assumptions })
  },

  getCachedTickers: async () => {
    return invoke<string[]>("ibkr_get_cached_tickers")
  },

  startScanner: async (subscription: ScannerSubscription) => {
    return invoke<void>("ibkr_start_scanner", { subscription })
  },

  stopScanner: async () => {
    return invoke<void>("ibkr_stop_scanner")
  },

  tracker: {
    add: async (params: {
      symbol: string
      source: TrackerSource
      sourceMeta?: Record<string, unknown> | null
      tags: StrategyTag[]
      notes?: string | null
    }) => {
      return invoke<TrackedTicker>("tracker_add", {
        symbol: params.symbol,
        source: params.source,
        sourceMeta: params.sourceMeta ?? null,
        tags: params.tags,
        notes: params.notes ?? null,
      })
    },

    remove: async (symbol: string) => {
      return invoke<void>("tracker_remove", { symbol })
    },

    list: async (status?: TrackerStatus) => {
      return invoke<TrackedTicker[]>("tracker_list", { status: status ?? null })
    },

    get: async (symbol: string) => {
      return invoke<TrackedTicker | null>("tracker_get", { symbol })
    },

    setTags: async (symbol: string, tags: StrategyTag[]) => {
      return invoke<TrackedTicker>("tracker_set_tags", { symbol, tags })
    },

    setStatus: async (symbol: string, status: TrackerStatus, inPlayUntil?: string | null) => {
      return invoke<TrackedTicker>("tracker_set_status", {
        symbol,
        status,
        inPlayUntil: inPlayUntil ?? null,
      })
    },

    fetchBars: async (symbol: string, barSize: BarSize, lookbackDays: number) => {
      return invoke<HistoricalBar[]>("tracker_fetch_bars", {
        symbol,
        barSize,
        lookbackDays,
      })
    },

    getNews: async (symbol: string, lookbackHours: number) => {
      return invoke<NewsItem[]>("tracker_get_news", { symbol, lookbackHours })
    },
  },
}
