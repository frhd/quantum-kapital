use serde::{Deserialize, Serialize};

/// Represents the authentication state for Google Sheets
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthState {
    pub authenticated: bool,
    pub user_email: Option<String>,
}

/// Configuration for Google Sheets export
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SheetsConfig {
    pub spreadsheet_id: Option<String>,
    pub spreadsheet_name: String,
}

impl Default for SheetsConfig {
    fn default() -> Self {
        Self {
            spreadsheet_id: None,
            spreadsheet_name: "Quantum Kapital Analysis".to_string(),
        }
    }
}

/// Represents a ticker's analysis data for export
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TickerAnalysisData {
    pub ticker: String,
    pub company_name: String,
    pub sector: Option<String>,
    pub market_cap: Option<String>,
    pub current_price: Option<f64>,
    pub pe_ratio: Option<f64>,
    pub eps: Option<f64>,
    pub historical_financials: Vec<HistoricalFinancial>,
    pub projections: ProjectionData,
    pub yearly_projections: Option<Vec<YearlyProjectionData>>, // NEW: Detailed year-by-year projections
    pub baseline_year: Option<u32>, // NEW: The baseline year for projections
}

/// Historical financial data for a single year
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoricalFinancial {
    pub year: String,
    pub revenue: Option<f64>,
    pub net_income: Option<f64>,
    pub eps: Option<f64>,
    pub growth_rate: Option<f64>,
}

/// Projection data for base, bear, and bull scenarios
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectionData {
    pub base: ScenarioProjection,
    pub bear: ScenarioProjection,
    pub bull: ScenarioProjection,
}

/// Individual scenario projection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioProjection {
    pub target_price: f64,
    pub upside_percent: f64,
    pub revenue_projection: f64,
    pub eps_projection: f64,
    pub timeline: String,
}

/// Yearly projection data with all scenarios for a single year
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YearlyProjectionData {
    pub year: u32,
    pub bear: YearlyScenarioData,
    pub base: YearlyScenarioData,
    pub bull: YearlyScenarioData,
}

/// Scenario data for a specific year
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YearlyScenarioData {
    pub revenue: f64,
    pub net_income: f64,
    pub eps: f64,
    pub share_price_low: f64,
    pub share_price_high: f64,
}

/// Dashboard summary data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardData {
    pub total_positions: usize,
    pub total_value: f64,
    pub analyzed_tickers: Vec<String>,
    pub last_updated: String,
}

/// Result of a sheet export operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportResult {
    pub success: bool,
    pub spreadsheet_id: String,
    pub spreadsheet_url: String,
    pub sheets_created: Vec<String>,
    pub message: String,
}

/// Error type for Google Sheets operations
#[derive(Debug, thiserror::Error)]
#[allow(clippy::enum_variant_names)]
pub enum SheetsError {
    #[error("Authentication failed: {0}")]
    AuthError(String),

    #[error("API error: {0}")]
    ApiError(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Data error: {0}")]
    DataError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Google API error: {0}")]
    GoogleError(String),
}

impl From<SheetsError> for String {
    fn from(error: SheetsError) -> Self {
        error.to_string()
    }
}
