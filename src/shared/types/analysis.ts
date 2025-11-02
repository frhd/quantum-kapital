// Financial projection for a single year
export interface FinancialProjection {
  year: number
  revenue: number              // in billions
  revenueGrowth: number        // percentage (e.g., 35.0 for 35%)
  netIncome: number            // in billions
  netIncomeGrowth: number | null // percentage, null for first year
  netIncomeMargins: number     // percentage (e.g., 17.0 for 17%)
  eps: number                  // dollars per share
  peLowEst: number
  peHighEst: number
  sharePriceLow: number
  sharePriceHigh: number
  valuationMethod: string      // "P/E" or "P/S" - indicates which method was used
  psLowEst?: number           // Price-to-Sales low (if P/S used)
  psHighEst?: number          // Price-to-Sales high (if P/S used)
  analystEpsEstimate?: number // Analyst consensus EPS estimate (if available)
}

// CAGR (Compound Annual Growth Rate) calculations
export interface CagrMetrics {
  revenue: number      // percentage
  sharePrice: number   // percentage
}

// Projections for a single year with bear/base/bull scenarios
export interface YearlyProjection {
  year: number
  bear: FinancialProjection
  base: FinancialProjection
  bull: FinancialProjection
}

// Complete projection results with baseline and forward projections
export interface ProjectionResults {
  baseline: FinancialProjection      // Most recent complete year (actual data)
  projections: YearlyProjection[]    // Future years with bear/base/bull scenarios
  cagr: ScenarioCagr                 // CAGR for each scenario
}

// Complete scenario projections (Bear/Base/Bull) - DEPRECATED, use ProjectionResults
export interface ScenarioProjections {
  bear: FinancialProjection[]
  base: FinancialProjection[]
  bull: FinancialProjection[]
  cagr: ScenarioCagr
}

export interface ScenarioCagr {
  bear: CagrMetrics
  base: CagrMetrics
  bull: CagrMetrics
}

// Historical financial data point
export interface HistoricalFinancial {
  year: number
  revenue: number
  netIncome: number
  eps: number
}

// Analyst estimate for a specific metric
export interface AnalystEstimate {
  year: number
  estimate: number
}

// Complete fundamental data for a security
export interface FundamentalData {
  symbol: string
  historical: HistoricalFinancial[]
  analystEstimates?: AnalystEstimates
  currentMetrics: CurrentMetrics
}

export interface AnalystEstimates {
  revenue: AnalystEstimate[]
  eps: AnalystEstimate[]
}

export interface CurrentMetrics {
  price: number
  peRatio: number
  sharesOutstanding: number // in millions
  name?: string
  exchange?: string
  marketCap?: string
  dividendYield?: number
}

// Assumptions for generating projections
export interface ProjectionAssumptions {
  years: number                    // number of years to project (default 5)
  bearRevenueGrowth: number        // percentage (e.g., 20.0 for 20%)
  baseRevenueGrowth: number        // percentage
  bullRevenueGrowth: number        // percentage
  bearMarginChange: number         // percentage points per year (can be negative)
  baseMarginChange: number         // percentage points per year
  bullMarginChange: number         // percentage points per year
  peLow: number                    // PE multiple low estimate (used when EPS > 0)
  peHigh: number                   // PE multiple high estimate (used when EPS > 0)
  psLow: number                    // Price-to-Sales low estimate (used when EPS < 0)
  psHigh: number                   // Price-to-Sales high estimate (used when EPS < 0)
  sharesGrowth: number             // annual change in shares (negative for buybacks)
}

export const defaultProjectionAssumptions: ProjectionAssumptions = {
  years: 5,
  bearRevenueGrowth: 20.0,
  baseRevenueGrowth: 35.0,
  bullRevenueGrowth: 50.0,
  bearMarginChange: -0.5,
  baseMarginChange: 0.5,
  bullMarginChange: 1.0,
  peLow: 50.0,
  peHigh: 60.0,
  psLow: 3.0,   // Conservative P/S for unprofitable companies
  psHigh: 8.0,  // Optimistic P/S for high-growth companies
  sharesGrowth: 0.0,
}

export type ScenarioType = 'bear' | 'base' | 'bull'
