export interface TickerSearchResult {
  symbol: string
  name: string
  exchange: string
  type: string
}

export interface TickerData {
  symbol: string
  name: string
  exchange: string
  type: string
  marketCap?: string
  pe?: number
  yield?: number
}
