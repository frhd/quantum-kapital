use serde::{Deserialize, Serialize};

/// Financial projection for a single year
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FinancialProjection {
    pub year: u32,
    pub revenue: f64,                   // in billions
    pub revenue_growth: f64,            // percentage (e.g., 35.0 for 35%)
    pub net_income: f64,                // in billions
    pub net_income_growth: Option<f64>, // percentage, None for first year
    pub net_income_margins: f64,        // percentage (e.g., 17.0 for 17%)
    pub eps: f64,                       // dollars per share
    pub pe_low_est: f64,
    pub pe_high_est: f64,
    pub share_price_low: f64,
    pub share_price_high: f64,
    pub valuation_method: String, // "P/E" or "P/S" - indicates which method was used
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ps_low_est: Option<f64>, // Price-to-Sales low (if P/S used)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ps_high_est: Option<f64>, // Price-to-Sales high (if P/S used)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub analyst_eps_estimate: Option<f64>, // Analyst consensus EPS estimate (if available)
}

/// CAGR (Compound Annual Growth Rate) calculations
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CagrMetrics {
    pub revenue: f64,     // percentage
    pub share_price: f64, // percentage
}

/// Projections for a single year with bear/base/bull scenarios
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct YearlyProjection {
    pub year: u32,
    pub bear: FinancialProjection,
    pub base: FinancialProjection,
    pub bull: FinancialProjection,
}

/// Complete projection results with baseline and forward projections
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectionResults {
    pub baseline: FinancialProjection, // Most recent complete year (actual data)
    pub projections: Vec<YearlyProjection>, // Future years with bear/base/bull scenarios
    pub cagr: ScenarioCagr,            // CAGR for each scenario
}

/// Complete scenario projections (Bear/Base/Bull) - DEPRECATED, use ProjectionResults
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioProjections {
    pub bear: Vec<FinancialProjection>,
    pub base: Vec<FinancialProjection>,
    pub bull: Vec<FinancialProjection>,
    pub cagr: ScenarioCagr,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScenarioCagr {
    pub bear: CagrMetrics,
    pub base: CagrMetrics,
    pub bull: CagrMetrics,
}

/// Historical financial data point
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoricalFinancial {
    pub year: u32,
    pub revenue: f64,
    pub net_income: f64,
    pub eps: f64,
}

/// Analyst estimate for a specific metric
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalystEstimate {
    pub year: u32,
    pub estimate: f64,
}

/// Complete fundamental data for a security
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FundamentalData {
    pub symbol: String,
    pub historical: Vec<HistoricalFinancial>,
    pub analyst_estimates: Option<AnalystEstimates>,
    pub current_metrics: CurrentMetrics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalystEstimates {
    pub revenue: Vec<AnalystEstimate>,
    pub eps: Vec<AnalystEstimate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentMetrics {
    pub price: f64,
    pub pe_ratio: f64,
    pub shares_outstanding: f64, // in millions
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exchange: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub market_cap: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dividend_yield: Option<f64>,
}

/// Assumptions for generating projections
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectionAssumptions {
    pub years: u32,               // number of years to project (default 5)
    pub bear_revenue_growth: f64, // percentage (e.g., 20.0 for 20%)
    pub base_revenue_growth: f64, // percentage
    pub bull_revenue_growth: f64, // percentage
    pub bear_margin_change: f64,  // percentage points per year (can be negative)
    pub base_margin_change: f64,  // percentage points per year
    pub bull_margin_change: f64,  // percentage points per year
    pub pe_low: f64,              // PE multiple low estimate (used when EPS > 0)
    pub pe_high: f64,             // PE multiple high estimate (used when EPS > 0)
    pub ps_low: f64,              // Price-to-Sales low estimate (used when EPS < 0)
    pub ps_high: f64,             // Price-to-Sales high estimate (used when EPS < 0)
    pub shares_growth: f64,       // annual change in shares (negative for buybacks)
}

impl Default for ProjectionAssumptions {
    fn default() -> Self {
        Self {
            years: 5,
            bear_revenue_growth: 20.0,
            base_revenue_growth: 35.0,
            bull_revenue_growth: 50.0,
            bear_margin_change: -0.5,
            base_margin_change: 0.5,
            bull_margin_change: 1.0,
            pe_low: 50.0,
            pe_high: 60.0,
            ps_low: 3.0,  // Conservative P/S for unprofitable companies
            ps_high: 8.0, // Optimistic P/S for high-growth companies
            shares_growth: 0.0,
        }
    }
}

/// Request to get fundamental data from IBKR
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FundamentalDataRequest {
    pub symbol: String,
    pub report_type: FundamentalReportType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FundamentalReportType {
    ReportsFinSummary,
    ReportsFinStatements,
    ReportsOwnership,
    ReportSnapshot,
    CalendarReport,
}

impl FundamentalReportType {
    #[allow(dead_code)]
    pub fn as_str(&self) -> &str {
        match self {
            Self::ReportsFinSummary => "ReportsFinSummary",
            Self::ReportsFinStatements => "ReportsFinStatements",
            Self::ReportsOwnership => "ReportsOwnership",
            Self::ReportSnapshot => "ReportSnapshot",
            Self::CalendarReport => "CalendarReport",
        }
    }
}
