use serde::{Deserialize, Serialize};

/// Financial projection for a single year
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinancialProjection {
    pub year: u32,
    pub revenue: f64,              // in billions
    pub revenue_growth: f64,       // percentage (e.g., 35.0 for 35%)
    pub net_income: f64,           // in billions
    pub net_income_growth: Option<f64>, // percentage, None for first year
    pub net_income_margins: f64,   // percentage (e.g., 17.0 for 17%)
    pub eps: f64,                  // dollars per share
    pub pe_low_est: f64,
    pub pe_high_est: f64,
    pub share_price_low: f64,
    pub share_price_high: f64,
}

/// CAGR (Compound Annual Growth Rate) calculations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CagrMetrics {
    pub revenue: f64,      // percentage
    pub share_price: f64,  // percentage
}

/// Complete scenario projections (Bear/Base/Bull)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioProjections {
    pub bear: Vec<FinancialProjection>,
    pub base: Vec<FinancialProjection>,
    pub bull: Vec<FinancialProjection>,
    pub cagr: ScenarioCagr,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioCagr {
    pub bear: CagrMetrics,
    pub base: CagrMetrics,
    pub bull: CagrMetrics,
}

/// Historical financial data point
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoricalFinancial {
    pub year: u32,
    pub revenue: f64,
    pub net_income: f64,
    pub eps: f64,
}

/// Analyst estimate for a specific metric
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalystEstimate {
    pub year: u32,
    pub estimate: f64,
}

/// Complete fundamental data for a security
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FundamentalData {
    pub symbol: String,
    pub historical: Vec<HistoricalFinancial>,
    pub analyst_estimates: Option<AnalystEstimates>,
    pub current_metrics: CurrentMetrics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalystEstimates {
    pub revenue: Vec<AnalystEstimate>,
    pub eps: Vec<AnalystEstimate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurrentMetrics {
    pub price: f64,
    pub pe_ratio: f64,
    pub shares_outstanding: f64, // in millions
}

/// Assumptions for generating projections
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectionAssumptions {
    pub years: u32,                    // number of years to project (default 5)
    pub bear_revenue_growth: f64,      // percentage (e.g., 20.0 for 20%)
    pub base_revenue_growth: f64,      // percentage
    pub bull_revenue_growth: f64,      // percentage
    pub bear_margin_change: f64,       // percentage points per year (can be negative)
    pub base_margin_change: f64,       // percentage points per year
    pub bull_margin_change: f64,       // percentage points per year
    pub pe_low: f64,                   // PE multiple low estimate
    pub pe_high: f64,                  // PE multiple high estimate
    pub shares_growth: f64,            // annual change in shares (negative for buybacks)
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
