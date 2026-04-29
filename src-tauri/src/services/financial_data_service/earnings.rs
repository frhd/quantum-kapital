use crate::ibkr::types::fundamentals::{AnalystEstimate, AnalystEstimates};
use crate::services::cache_service::CacheService;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::error::Error;
use tracing::info;

#[derive(Debug, Serialize, Deserialize)]
#[allow(dead_code)]
pub(super) struct AlphaVantageEarnings {
    #[serde(default)]
    pub(super) symbol: Option<String>,
    #[serde(rename = "annualEarnings", default)]
    pub(super) annual_earnings: Vec<AnnualEarning>,
    #[serde(rename = "quarterlyEarnings", default)]
    pub(super) quarterly_earnings: Option<Vec<QuarterlyEarning>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct AnnualEarning {
    #[serde(rename = "fiscalDateEnding")]
    pub(super) fiscal_date_ending: String,
    #[serde(rename = "reportedEPS")]
    pub(super) reported_eps: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct QuarterlyEarning {
    #[serde(rename = "fiscalDateEnding")]
    pub(super) fiscal_date_ending: String,
    #[serde(rename = "estimatedEPS")]
    pub(super) estimated_eps: Option<String>,
}

pub(super) async fn fetch_earnings(
    client: &Client,
    api_key: &str,
    base_url: &str,
    cache: &Option<CacheService>,
    symbol: &str,
) -> Result<AlphaVantageEarnings, Box<dyn Error + Send + Sync>> {
    let cache_key = format!("{}_earnings", symbol.to_uppercase());

    if let Some(ref c) = cache {
        if let Ok(cached_data) = c.read::<AlphaVantageEarnings>(&cache_key) {
            info!("Using cached earnings data for {}", symbol);
            return Ok(cached_data);
        }
    }

    info!("Fetching earnings data from API for {}", symbol);
    let url = format!(
        "{}?function=EARNINGS&symbol={}&apikey={}",
        base_url, symbol, api_key
    );

    let response = client.get(&url).send().await?;

    if !response.status().is_success() {
        return Err(format!("API request failed: {}", response.status()).into());
    }

    let json: Value = response.json().await?;
    super::check_api_error(&json)?;

    let earnings: AlphaVantageEarnings = serde_json::from_value(json)?;

    if let Some(ref c) = cache {
        let _ = c.write(&cache_key, &earnings);
    }

    Ok(earnings)
}

pub(super) fn process_analyst_estimates(
    earnings: &AlphaVantageEarnings,
) -> Option<AnalystEstimates> {
    let quarterly = earnings.quarterly_earnings.as_ref()?;

    if quarterly.is_empty() {
        return None;
    }

    use std::collections::HashMap;
    let mut quarterly_by_year: HashMap<u32, Vec<f64>> = HashMap::new();

    for q in quarterly.iter() {
        if let Some(year) = q
            .fiscal_date_ending
            .split('-')
            .next()
            .and_then(|y| y.parse::<u32>().ok())
        {
            if let Some(estimate) = q
                .estimated_eps
                .as_ref()
                .and_then(|eps_str| eps_str.parse::<f64>().ok())
            {
                quarterly_by_year.entry(year).or_default().push(estimate);
            }
        }
    }

    let mut annual_eps: Vec<AnalystEstimate> = quarterly_by_year
        .into_iter()
        .map(|(year, quarters)| {
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

    annual_eps.sort_by_key(|e| e.year);

    if annual_eps.is_empty() {
        None
    } else {
        info!(
            "Processed {} annual EPS estimates from quarterly data",
            annual_eps.len()
        );
        Some(AnalystEstimates {
            revenue: vec![],
            eps: annual_eps,
        })
    }
}
