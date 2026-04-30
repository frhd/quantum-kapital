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
  Quote,
} from "../types"
import type {
  Alert,
  AlertKind,
  TrackedTicker,
  TrackerSource,
  TrackerStatus,
  StrategyTag,
  NewsItem,
  HistoricalBar,
  BarSize,
  MorningPack,
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

  getQuote: async (symbol: string) => {
    return invoke<Quote>("ibkr_get_quote", { symbol })
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

    setStatus: async (
      symbol: string,
      status: TrackerStatus,
      inPlayUntil?: string | null,
      coolDownUntil?: string | null,
    ) => {
      return invoke<TrackedTicker>("tracker_set_status", {
        symbol,
        status,
        inPlayUntil: inPlayUntil ?? null,
        coolDownUntil: coolDownUntil ?? null,
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

    /** Phase 20 — fetch the persisted morning pack. Pass a date (ISO `YYYY-MM-DD`)
     *  to fetch a specific day; omit to get the most recent pack. */
    getMorningPack: async (date?: string) => {
      return invoke<MorningPack | null>("tracker_get_morning_pack", { date: date ?? null })
    },

    /** Phase 21 — read a slice of the alert feed. All filters are AND-combined. */
    listAlerts: async (params?: {
      limit?: number
      offset?: number
      since?: string | null
      kind?: AlertKind | null
      onlyUnseen?: boolean
    }) => {
      return invoke<Alert[]>("tracker_list_alerts", {
        limit: params?.limit ?? null,
        offset: params?.offset ?? null,
        since: params?.since ?? null,
        kind: params?.kind ?? null,
        onlyUnseen: params?.onlyUnseen ?? null,
      })
    },

    /** Phase 21 — flip the listed ids' `seen` to true. Returns the number
     *  of rows actually flipped (already-seen / unknown ids contribute 0). */
    markAlertsSeen: async (ids: number[]) => {
      return invoke<number>("tracker_mark_alerts_seen", { ids })
    },
  },
}
