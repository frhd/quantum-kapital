use crate::ibkr::types::fundamentals::CurrentMetrics;
use crate::middleware::AlphaVantageRateLimiter;
use crate::services::cache_service::CacheService;
use serde::{Deserialize, Serialize};
use std::error::Error;

use super::AvHttp;

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
    http: &dyn AvHttp,
    rate_limiter: Option<&AlphaVantageRateLimiter>,
    api_key: &str,
    base_url: &str,
    cache: &Option<CacheService>,
    symbol: &str,
) -> Result<AlphaVantageOverview, Box<dyn Error + Send + Sync>> {
    super::fetch_av_function(
        http,
        rate_limiter,
        api_key,
        base_url,
        cache,
        symbol,
        "OVERVIEW",
        "overview",
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
        // Alpha Vantage OVERVIEW does not carry a real current price.
        // The live-quote path (ibkr_get_quote) owns this concern.
        price: None,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_current_metrics_does_not_alias_52week_high_as_price() {
        let overview = AlphaVantageOverview {
            symbol: Some("AAPL".into()),
            name: Some("Apple".into()),
            exchange: Some("NASDAQ".into()),
            market_capitalization: Some("3000000000000".into()),
            pe_ratio: Some("30.0".into()),
            shares_outstanding: Some("15000000000".into()),
            week_52_high: Some("202.49".into()),
            dividend_yield: Some("0.005".into()),
        };

        let metrics = process_current_metrics(&overview);

        assert!(metrics.price.is_none(), "OVERVIEW must not populate price");
        assert!((metrics.pe_ratio - 30.0).abs() < 1e-9);
        assert_eq!(metrics.market_cap.as_deref(), Some("3.0T"));
    }
}
