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
  price?: number
  change?: number
  changePercent?: number
  volume?: number
  marketCap?: string
  pe?: number
  yield?: number
}
