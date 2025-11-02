use crate::ibkr::types::fundamentals::{
    AnalystEstimate, AnalystEstimates, CurrentMetrics, FundamentalData, HistoricalFinancial,
};
use crate::services::cache_service::CacheService;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::error::Error;
use std::path::PathBuf;
use tracing::{debug, info, warn};

/// Service for fetching fundamental data from Alpha Vantage API
pub struct FinancialDataService {
    client: Client,
    api_key: String,
    base_url: String,
    cache: Option<CacheService>,
}

// Alpha Vantage API response structures

#[derive(Debug, Serialize, Deserialize)]
#[allow(dead_code)]
struct AlphaVantageOverview {
    #[serde(rename = "Symbol")]
    symbol: Option<String>,
    #[serde(rename = "Name")]
    name: Option<String>,
    #[serde(rename = "Exchange")]
    exchange: Option<String>,
    #[serde(rename = "MarketCapitalization")]
    market_capitalization: Option<String>,
    #[serde(rename = "PERatio")]
    pe_ratio: Option<String>,
    #[serde(rename = "SharesOutstanding")]
    shares_outstanding: Option<String>,
    #[serde(rename = "52WeekHigh")]
    week_52_high: Option<String>,
    #[serde(rename = "DividendYield")]
    dividend_yield: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[allow(dead_code)]
struct AlphaVantageIncomeStatement {
    #[serde(default)]
    symbol: Option<String>,
    #[serde(rename = "annualReports", default)]
    annual_reports: Vec<AnnualReport>,
}

#[derive(Debug, Serialize, Deserialize)]
struct AnnualReport {
    #[serde(rename = "fiscalDateEnding")]
    fiscal_date_ending: String,
    #[serde(rename = "totalRevenue")]
    total_revenue: Option<String>,
    #[serde(rename = "netIncome")]
    net_income: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[allow(dead_code)]
struct AlphaVantageEarnings {
    #[serde(default)]
    symbol: Option<String>,
    #[serde(rename = "annualEarnings", default)]
    annual_earnings: Vec<AnnualEarning>,
    #[serde(rename = "quarterlyEarnings", default)]
    quarterly_earnings: Option<Vec<QuarterlyEarning>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct AnnualEarning {
    #[serde(rename = "fiscalDateEnding")]
    fiscal_date_ending: String,
    #[serde(rename = "reportedEPS")]
    reported_eps: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct QuarterlyEarning {
    #[serde(rename = "fiscalDateEnding")]
    fiscal_date_ending: String,
    #[serde(rename = "estimatedEPS")]
    estimated_eps: Option<String>,
}

impl FinancialDataService {
    /// Check if the API response contains an error message
    fn check_api_error(json: &Value) -> Result<(), Box<dyn Error + Send + Sync>> {
        // Check for "Error Message" field
        if let Some(error_msg) = json.get("Error Message").and_then(|v| v.as_str()) {
            return Err(format!("Alpha Vantage API error: {error_msg}").into());
        }

        // Check for "Note" field (rate limit warning)
        if let Some(note) = json.get("Note").and_then(|v| v.as_str()) {
            warn!("Alpha Vantage API note: {}", note);
            return Err("API rate limit reached. Please try again later.".into());
        }

        // Check for "Information" field (another type of message)
        if let Some(info) = json.get("Information").and_then(|v| v.as_str()) {
            warn!("Alpha Vantage API information: {}", info);
            return Err(format!("Alpha Vantage API: {info}").into());
        }

        Ok(())
    }

    /// Creates a new FinancialDataService instance for Alpha Vantage
    pub fn new(api_key: String) -> Self {
        Self::with_cache_dir(api_key, "cache/alphavantage")
    }

    /// Creates a new FinancialDataService instance with a custom cache directory
    pub fn with_cache_dir(api_key: String, cache_dir: impl Into<PathBuf>) -> Self {
        let cache = CacheService::new(cache_dir.into())
            .map_err(|e| {
                debug!("Failed to initialize cache: {}", e);
                e
            })
            .ok();

        if cache.is_some() {
            info!("Alpha Vantage cache enabled at cache/alphavantage");
        } else {
            info!("Alpha Vantage cache disabled");
        }

        Self {
            client: Client::new(),
            api_key,
            base_url: "https://www.alphavantage.co/query".to_string(),
            cache,
        }
    }

    /// Fetches fundamental data for a given symbol
    pub async fn fetch_fundamental_data(
        &self,
        symbol: &str,
    ) -> Result<FundamentalData, Box<dyn Error + Send + Sync>> {
        // Fetch all required data in parallel
        let (overview, income_statement, earnings) = tokio::try_join!(
            self.fetch_overview(symbol),
            self.fetch_income_statement(symbol),
            self.fetch_earnings(symbol)
        )?;

        // Process historical data (combine income statement and earnings)
        let historical = self.process_historical_data(&income_statement, &earnings);

        // Validate we have at least some historical data
        if historical.is_empty() {
            return Err(format!(
                "No historical financial data available for {symbol}. This ticker may be too new or not have sufficient financial reporting history."
            ).into());
        }

        // Get current metrics from overview
        let current_metrics = self.process_current_metrics(&overview);

        // Process analyst estimates from quarterly earnings
        let analyst_estimates = self.process_analyst_estimates(&earnings);

        Ok(FundamentalData {
            symbol: symbol.to_uppercase(),
            historical,
            analyst_estimates,
            current_metrics,
        })
    }

    async fn fetch_overview(
        &self,
        symbol: &str,
    ) -> Result<AlphaVantageOverview, Box<dyn Error + Send + Sync>> {
        let cache_key = format!("{}_overview", symbol.to_uppercase());

        // Try to read from cache first
        if let Some(ref cache) = self.cache {
            if let Ok(cached_data) = cache.read::<AlphaVantageOverview>(&cache_key) {
                info!("Using cached overview data for {}", symbol);
                return Ok(cached_data);
            }
        }

        // Fetch from API
        info!("Fetching overview data from API for {}", symbol);
        let url = format!(
            "{}?function=OVERVIEW&symbol={}&apikey={}",
            self.base_url, symbol, self.api_key
        );

        let response = self.client.get(&url).send().await?;

        if !response.status().is_success() {
            return Err(format!("API request failed: {}", response.status()).into());
        }

        // Parse as generic JSON first to check for errors
        let json: Value = response.json().await?;
        Self::check_api_error(&json)?;

        // Deserialize to the specific type
        let overview: AlphaVantageOverview = serde_json::from_value(json)?;

        // Write to cache
        if let Some(ref cache) = self.cache {
            let _ = cache.write(&cache_key, &overview);
        }

        Ok(overview)
    }

    async fn fetch_income_statement(
        &self,
        symbol: &str,
    ) -> Result<AlphaVantageIncomeStatement, Box<dyn Error + Send + Sync>> {
        let cache_key = format!("{}_income_statement", symbol.to_uppercase());

        // Try to read from cache first
        if let Some(ref cache) = self.cache {
            if let Ok(cached_data) = cache.read::<AlphaVantageIncomeStatement>(&cache_key) {
                info!("Using cached income statement data for {}", symbol);
                return Ok(cached_data);
            }
        }

        // Fetch from API
        info!("Fetching income statement data from API for {}", symbol);
        let url = format!(
            "{}?function=INCOME_STATEMENT&symbol={}&apikey={}",
            self.base_url, symbol, self.api_key
        );

        let response = self.client.get(&url).send().await?;

        if !response.status().is_success() {
            return Err(format!("API request failed: {}", response.status()).into());
        }

        // Parse as generic JSON first to check for errors
        let json: Value = response.json().await?;
        Self::check_api_error(&json)?;

        // Deserialize to the specific type
        let statement: AlphaVantageIncomeStatement = serde_json::from_value(json)?;

        // Write to cache
        if let Some(ref cache) = self.cache {
            let _ = cache.write(&cache_key, &statement);
        }

        Ok(statement)
    }

    async fn fetch_earnings(
        &self,
        symbol: &str,
    ) -> Result<AlphaVantageEarnings, Box<dyn Error + Send + Sync>> {
        let cache_key = format!("{}_earnings", symbol.to_uppercase());

        // Try to read from cache first
        if let Some(ref cache) = self.cache {
            if let Ok(cached_data) = cache.read::<AlphaVantageEarnings>(&cache_key) {
                info!("Using cached earnings data for {}", symbol);
                return Ok(cached_data);
            }
        }

        // Fetch from API
        info!("Fetching earnings data from API for {}", symbol);
        let url = format!(
            "{}?function=EARNINGS&symbol={}&apikey={}",
            self.base_url, symbol, self.api_key
        );

        let response = self.client.get(&url).send().await?;

        if !response.status().is_success() {
            return Err(format!("API request failed: {}", response.status()).into());
        }

        // Parse as generic JSON first to check for errors
        let json: Value = response.json().await?;
        Self::check_api_error(&json)?;

        // Deserialize to the specific type
        let earnings: AlphaVantageEarnings = serde_json::from_value(json)?;

        // Write to cache
        if let Some(ref cache) = self.cache {
            let _ = cache.write(&cache_key, &earnings);
        }

        Ok(earnings)
    }

    fn process_historical_data(
        &self,
        income_statement: &AlphaVantageIncomeStatement,
        earnings: &AlphaVantageEarnings,
    ) -> Vec<HistoricalFinancial> {
        // Parse ALL reports first, then take the most recent 5 years
        let mut historical: Vec<HistoricalFinancial> = income_statement
            .annual_reports
            .iter()
            .filter_map(|report| {
                // Extract year from fiscal date ending (format: "YYYY-MM-DD")
                let year = report
                    .fiscal_date_ending
                    .split('-')
                    .next()
                    .and_then(|y| y.parse::<u32>().ok())?;

                // Parse revenue (string to f64, convert to billions)
                let revenue = report
                    .total_revenue
                    .as_ref()
                    .and_then(|r| r.parse::<f64>().ok())
                    .map(|r| r / 1_000_000_000.0)?;

                // Parse net income (string to f64, convert to billions)
                let net_income = report
                    .net_income
                    .as_ref()
                    .and_then(|n| n.parse::<f64>().ok())
                    .map(|n| n / 1_000_000_000.0)?;

                // Find matching EPS from earnings data
                let eps = earnings
                    .annual_earnings
                    .iter()
                    .find(|e| e.fiscal_date_ending == report.fiscal_date_ending)
                    .and_then(|e| e.reported_eps.as_ref())
                    .and_then(|eps_str| eps_str.parse::<f64>().ok())
                    .unwrap_or(0.0);

                Some(HistoricalFinancial {
                    year,
                    revenue,
                    net_income,
                    eps,
                })
            })
            .collect();

        // CRITICAL: Sort by year ascending (oldest to newest)
        historical.sort_by_key(|h| h.year);

        // Take only the LAST 5 years (most recent)
        // This ensures we don't use old data if API returns 10+ years
        let start_idx = historical.len().saturating_sub(5);
        historical[start_idx..].to_vec()
    }

    fn process_current_metrics(&self, overview: &AlphaVantageOverview) -> CurrentMetrics {
        // Parse P/E ratio
        let pe_ratio = overview
            .pe_ratio
            .as_ref()
            .and_then(|pe| pe.parse::<f64>().ok())
            .unwrap_or(0.0);

        // Parse shares outstanding (convert to millions)
        let shares_outstanding = overview
            .shares_outstanding
            .as_ref()
            .and_then(|s| s.parse::<f64>().ok())
            .map(|s| s / 1_000_000.0)
            .unwrap_or(0.0);

        // Use 52-week high as approximate current price
        // (Alpha Vantage OVERVIEW doesn't include real-time price)
        let price = overview
            .week_52_high
            .as_ref()
            .and_then(|p| p.parse::<f64>().ok())
            .unwrap_or(0.0);

        // Parse dividend yield (convert from percentage string like "0.42" to 0.42)
        let dividend_yield = overview
            .dividend_yield
            .as_ref()
            .and_then(|dy| dy.parse::<f64>().ok());

        // Format market cap (already a string from API, e.g., "2800000000000" -> "2.8T")
        let market_cap = overview.market_capitalization.as_ref().map(|mc_str| {
            if let Ok(mc) = mc_str.parse::<f64>() {
                Self::format_market_cap(mc)
            } else {
                mc_str.clone()
            }
        });

        CurrentMetrics {
            price,
            pe_ratio,
            shares_outstanding,
            name: overview.name.clone(),
            exchange: overview.exchange.clone(),
            market_cap,
            dividend_yield,
        }
    }

    fn format_market_cap(value: f64) -> String {
        if value >= 1_000_000_000_000.0 {
            format!("{:.1}T", value / 1_000_000_000_000.0)
        } else if value >= 1_000_000_000.0 {
            format!("{:.1}B", value / 1_000_000_000.0)
        } else if value >= 1_000_000.0 {
            format!("{:.1}M", value / 1_000_000.0)
        } else {
            format!("{value:.0}")
        }
    }

    fn process_analyst_estimates(
        &self,
        earnings: &AlphaVantageEarnings,
    ) -> Option<AnalystEstimates> {
        let quarterly = earnings.quarterly_earnings.as_ref()?;

        if quarterly.is_empty() {
            return None;
        }

        // Extract quarterly EPS estimates and group by year
        use std::collections::HashMap;
        let mut quarterly_by_year: HashMap<u32, Vec<f64>> = HashMap::new();

        for q in quarterly.iter() {
            // Extract year from fiscal date ending (format: "YYYY-MM-DD")
            if let Some(year) = q
                .fiscal_date_ending
                .split('-')
                .next()
                .and_then(|y| y.parse::<u32>().ok())
            {
                // Parse estimated EPS
                if let Some(estimate) = q
                    .estimated_eps
                    .as_ref()
                    .and_then(|eps_str| eps_str.parse::<f64>().ok())
                {
                    quarterly_by_year.entry(year).or_default().push(estimate);
                }
            }
        }

        // Aggregate quarterly estimates to annual (sum all quarters for each year)
        let mut annual_eps: Vec<AnalystEstimate> = quarterly_by_year
            .into_iter()
            .map(|(year, quarters)| {
                // Log if we don't have all 4 quarters of data
                if quarters.len() != 4 {
                    info!(
                        "Year {} has {} quarters of EPS estimates (partial year)",
                        year,
                        quarters.len()
                    );
                }

                let annual_estimate = quarters.iter().sum();
                AnalystEstimate {
                    year,
                    estimate: annual_estimate,
                }
            })
            .collect();

        // Sort by year
        annual_eps.sort_by_key(|e| e.year);

        if annual_eps.is_empty() {
            None
        } else {
            info!(
                "Processed {} annual EPS estimates from quarterly data",
                annual_eps.len()
            );
            Some(AnalystEstimates {
                revenue: vec![], // Alpha Vantage doesn't provide revenue estimates in EARNINGS endpoint
                eps: annual_eps,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore] // Requires API key
    async fn test_fetch_fundamental_data() {
        let api_key =
            std::env::var("ALPHA_VANTAGE_API_KEY").expect("ALPHA_VANTAGE_API_KEY not set");
        let service = FinancialDataService::new(api_key);

        let result = service.fetch_fundamental_data("AAPL").await;
        assert!(result.is_ok());

        let data = result.unwrap();
        assert_eq!(data.symbol, "AAPL");
        assert!(!data.historical.is_empty());
        assert!(data.current_metrics.pe_ratio > 0.0);
    }
}
