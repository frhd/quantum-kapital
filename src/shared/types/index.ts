export interface ConnectionConfig {
  host: string
  port: number
  client_id: number
}

export interface ConnectionStatus {
  connected: boolean
  server_time: string | null
  client_id: number
}

export interface AccountSummary {
  account: string
  tag: string
  value: string
  currency: string
}

export interface Position {
  account: string
  symbol: string
  position: number
  average_cost: number
  market_price: number
  market_value: number
  unrealized_pnl: number
  realized_pnl: number
  contract_type: string
  currency: string
  exchange: string
  local_symbol: string
}

export interface OrderRequest {
  symbol: string
  action: "Buy" | "Sell"
  quantity: number
  order_type: "Market" | "Limit" | "Stop" | "StopLimit"
  price?: number
}

export interface DailyPnL {
  account: string
  daily_pnl: number
  unrealized_pnl: number | null
  realized_pnl: number | null
}

export type SecurityType =
  | "Stock"
  | "Option"
  | "Future"
  | "Forex"
  | "Combo"
  | "Warrant"
  | "Bond"
  | "Commodity"
  | "News"
  | "Fund"

export interface ContractDetails {
  symbol: string
  sec_type: SecurityType
  exchange: string
  primary_exchange: string
  currency: string
  local_symbol: string
  trading_class: string
  contract_id: number
  min_tick: number
  multiplier: string
  price_magnifier: number
}

export interface ScannerSubscription {
  number_of_rows: number
  instrument: string
  location_code: string
  scan_code: string
  above_price?: number
  below_price?: number
  above_volume?: number
  market_cap_above?: number
  market_cap_below?: number
  /** IBKR `industryLike` filter (e.g. "Semiconductors"). Omit for broad-market scans. */
  industry_filter?: string
}

export interface ScannerData {
  rank: number
  contract: ContractDetails
  leg: string
}

// Re-export analysis types
export * from "./analysis"
