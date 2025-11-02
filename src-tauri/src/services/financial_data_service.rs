use crate::ibkr::types::fundamentals::{
    AnalystEstimate, AnalystEstimates, CurrentMetrics, FundamentalData, HistoricalFinancial,
};
use crate::services::cache_service::CacheService;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::path::PathBuf;
use tracing::{debug, info};

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
    #[serde(rename = "MarketCapitalization")]
    market_capitalization: Option<String>,
    #[serde(rename = "PERatio")]
    pe_ratio: Option<String>,
    #[serde(rename = "SharesOutstanding")]
    shares_outstanding: Option<String>,
    #[serde(rename = "52WeekHigh")]
    week_52_high: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[allow(dead_code)]
struct AlphaVantageIncomeStatement {
    symbol: String,
    #[serde(rename = "annualReports")]
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
    symbol: String,
    #[serde(rename = "annualEarnings")]
    annual_earnings: Vec<AnnualEarning>,
    #[serde(rename = "quarterlyEarnings")]
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

        let overview: AlphaVantageOverview = response.json().await?;

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

        let statement: AlphaVantageIncomeStatement = response.json().await?;

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

        let earnings: AlphaVantageEarnings = response.json().await?;

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
        // Take last 5 years
        let annual_reports: Vec<_> = income_statement.annual_reports.iter().take(5).collect();

        annual_reports
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
            .collect()
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

        CurrentMetrics {
            price,
            pe_ratio,
            shares_outstanding,
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

        // Extract future quarters with estimates
        let eps: Vec<AnalystEstimate> = quarterly
            .iter()
            .filter_map(|q| {
                let year = q
                    .fiscal_date_ending
                    .split('-')
                    .next()
                    .and_then(|y| y.parse::<u32>().ok())?;

                let estimate = q
                    .estimated_eps
                    .as_ref()
                    .and_then(|eps_str| eps_str.parse::<f64>().ok())?;

                Some(AnalystEstimate { year, estimate })
            })
            .collect();

        if eps.is_empty() {
            None
        } else {
            Some(AnalystEstimates {
                revenue: vec![], // Alpha Vantage doesn't provide revenue estimates in EARNINGS endpoint
                eps,
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
