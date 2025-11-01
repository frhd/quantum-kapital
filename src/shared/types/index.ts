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

// Re-export analysis types
export * from "./analysis"