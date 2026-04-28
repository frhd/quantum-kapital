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
  ScannerSubscription
} from "../types"

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
  }
}
