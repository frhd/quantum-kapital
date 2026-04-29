use crate::ibkr::types::fundamentals::HistoricalFinancial;
use crate::services::cache_service::CacheService;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::error::Error;
use tracing::info;

use super::earnings::AlphaVantageEarnings;

#[derive(Debug, Serialize, Deserialize)]
#[allow(dead_code)]
pub(super) struct AlphaVantageIncomeStatement {
    #[serde(default)]
    pub(super) symbol: Option<String>,
    #[serde(rename = "annualReports", default)]
    pub(super) annual_reports: Vec<AnnualReport>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct AnnualReport {
    #[serde(rename = "fiscalDateEnding")]
    pub(super) fiscal_date_ending: String,
    #[serde(rename = "totalRevenue")]
    pub(super) total_revenue: Option<String>,
    #[serde(rename = "netIncome")]
    pub(super) net_income: Option<String>,
}

pub(super) async fn fetch_income_statement(
    client: &Client,
    api_key: &str,
    base_url: &str,
    cache: &Option<CacheService>,
    symbol: &str,
) -> Result<AlphaVantageIncomeStatement, Box<dyn Error + Send + Sync>> {
    let cache_key = format!("{}_income_statement", symbol.to_uppercase());

    if let Some(ref c) = cache {
        if let Ok(cached_data) = c.read::<AlphaVantageIncomeStatement>(&cache_key) {
            info!("Using cached income statement data for {}", symbol);
            return Ok(cached_data);
        }
    }

    info!("Fetching income statement data from API for {}", symbol);
    let url = format!(
        "{}?function=INCOME_STATEMENT&symbol={}&apikey={}",
        base_url, symbol, api_key
    );

    let response = client.get(&url).send().await?;

    if !response.status().is_success() {
        return Err(format!("API request failed: {}", response.status()).into());
    }

    let json: Value = response.json().await?;
    super::check_api_error(&json)?;

    let statement: AlphaVantageIncomeStatement = serde_json::from_value(json)?;

    if let Some(ref c) = cache {
        let _ = c.write(&cache_key, &statement);
    }

    Ok(statement)
}

pub(super) fn process_historical_data(
    income_statement: &AlphaVantageIncomeStatement,
    earnings: &AlphaVantageEarnings,
) -> Vec<HistoricalFinancial> {
    let mut historical: Vec<HistoricalFinancial> = income_statement
        .annual_reports
        .iter()
        .filter_map(|report| {
            let year = report
                .fiscal_date_ending
                .split('-')
                .next()
                .and_then(|y| y.parse::<u32>().ok())?;

            let revenue = report
                .total_revenue
                .as_ref()
                .and_then(|r| r.parse::<f64>().ok())
                .map(|r| r / 1_000_000_000.0)?;

            let net_income = report
                .net_income
                .as_ref()
                .and_then(|n| n.parse::<f64>().ok())
                .map(|n| n / 1_000_000_000.0)?;

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
    let start_idx = historical.len().saturating_sub(5);
    historical[start_idx..].to_vec()
}
