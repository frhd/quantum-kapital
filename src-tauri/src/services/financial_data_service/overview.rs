use crate::ibkr::types::fundamentals::CurrentMetrics;
use crate::services::cache_service::CacheService;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::error::Error;

#[derive(Debug, Serialize, Deserialize)]
#[allow(dead_code)]
pub(super) struct AlphaVantageOverview {
    #[serde(rename = "Symbol")]
    pub(super) symbol: Option<String>,
    #[serde(rename = "Name")]
    pub(super) name: Option<String>,
    #[serde(rename = "Exchange")]
    pub(super) exchange: Option<String>,
    #[serde(rename = "MarketCapitalization")]
    pub(super) market_capitalization: Option<String>,
    #[serde(rename = "PERatio")]
    pub(super) pe_ratio: Option<String>,
    #[serde(rename = "SharesOutstanding")]
    pub(super) shares_outstanding: Option<String>,
    #[serde(rename = "52WeekHigh")]
    pub(super) week_52_high: Option<String>,
    #[serde(rename = "DividendYield")]
    pub(super) dividend_yield: Option<String>,
}

pub(super) async fn fetch_overview(
    client: &Client,
    api_key: &str,
    base_url: &str,
    cache: &Option<CacheService>,
    symbol: &str,
) -> Result<AlphaVantageOverview, Box<dyn Error + Send + Sync>> {
    super::fetch_av_function(
        client, api_key, base_url, cache, symbol, "OVERVIEW", "overview",
    )
    .await
}

pub(super) fn process_current_metrics(overview: &AlphaVantageOverview) -> CurrentMetrics {
    let pe_ratio = overview
        .pe_ratio
        .as_ref()
        .and_then(|pe| pe.parse::<f64>().ok())
        .unwrap_or(0.0);

    let shares_outstanding = overview
        .shares_outstanding
        .as_ref()
        .and_then(|s| s.parse::<f64>().ok())
        .map(|s| s / 1_000_000.0)
        .unwrap_or(0.0);

    let price = overview
        .week_52_high
        .as_ref()
        .and_then(|p| p.parse::<f64>().ok())
        .unwrap_or(0.0);

    let dividend_yield = overview
        .dividend_yield
        .as_ref()
        .and_then(|dy| dy.parse::<f64>().ok());

    let market_cap = overview.market_capitalization.as_ref().map(|mc_str| {
        if let Ok(mc) = mc_str.parse::<f64>() {
            format_market_cap(mc)
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

pub(super) fn format_market_cap(value: f64) -> String {
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
