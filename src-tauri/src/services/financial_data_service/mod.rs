use crate::ibkr::types::news::NewsItem;
use crate::services::cache_service::CacheService;
use crate::services::news_interpreter::NewsInterpreter;
use crate::storage::Db;
use reqwest::Client;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use std::error::Error;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{debug, info, warn};

mod earnings;
mod income;
mod overview;

pub mod news;

#[cfg(test)]
mod news_tests;

/// Service for fetching fundamental data from Alpha Vantage API
pub struct FinancialDataService {
    client: Client,
    api_key: String,
    base_url: String,
    cache: Option<CacheService>,
    db: Option<Arc<Db>>,
    news_interpreter: Option<Arc<NewsInterpreter>>,
}

/// Fetch a single Alpha Vantage `function=...` payload, hitting the local
/// JSON cache first and falling back to the live API. Cache key is namespaced
/// per (symbol, suffix) so the three callers — OVERVIEW, INCOME_STATEMENT,
/// EARNINGS — don't collide on disk.
pub(super) async fn fetch_av_function<T>(
    client: &Client,
    api_key: &str,
    base_url: &str,
    cache: &Option<CacheService>,
    symbol: &str,
    function: &str,
    cache_suffix: &str,
) -> Result<T, Box<dyn Error + Send + Sync>>
where
    T: DeserializeOwned + Serialize,
{
    let cache_key = format!("{}_{}", symbol.to_uppercase(), cache_suffix);

    if let Some(ref c) = cache {
        if let Ok(cached) = c.read::<T>(&cache_key) {
            info!("Using cached {function} data for {symbol}");
            return Ok(cached);
        }
    }

    info!("Fetching {function} data from API for {symbol}");
    let url = format!("{base_url}?function={function}&symbol={symbol}&apikey={api_key}");

    let response = client.get(&url).send().await?;
    if !response.status().is_success() {
        return Err(format!("API request failed: {}", response.status()).into());
    }

    let json: Value = response.json().await?;
    check_api_error(&json)?;

    let parsed: T = serde_json::from_value(json)?;
    if let Some(ref c) = cache {
        let _ = c.write(&cache_key, &parsed);
    }
    Ok(parsed)
}

/// Check if the API response contains an error message
pub(super) fn check_api_error(json: &Value) -> Result<(), Box<dyn Error + Send + Sync>> {
    if let Some(error_msg) = json.get("Error Message").and_then(|v| v.as_str()) {
        return Err(format!("Alpha Vantage API error: {error_msg}").into());
    }

    if let Some(note) = json.get("Note").and_then(|v| v.as_str()) {
        warn!("Alpha Vantage API note: {}", note);
        return Err("API rate limit reached. Please try again later.".into());
    }

    if let Some(info) = json.get("Information").and_then(|v| v.as_str()) {
        warn!("Alpha Vantage API information: {}", info);
        return Err(format!("Alpha Vantage API: {info}").into());
    }

    Ok(())
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
            db: None,
            news_interpreter: None,
        }
    }

    /// Attach a SQLite handle so news fetches can read/write `news_cache`.
    pub fn with_db(mut self, db: Arc<Db>) -> Self {
        self.db = Some(db);
        self
    }

    /// Attach an LLM-backed news interpreter (Phase 19). When wired,
    /// each successful `fetch_news_sentiment` triggers a best-effort
    /// `interpret(symbol)` that lands a `NewsVerdict` in
    /// `news_cache.news_verdict_json`. Interpreter failures are logged
    /// but never propagate — the news fetch itself stays unaffected.
    pub fn with_news_interpreter(mut self, interpreter: Arc<NewsInterpreter>) -> Self {
        self.news_interpreter = Some(interpreter);
        self
    }

    /// Fetch ticker-tagged news + sentiment from Alpha Vantage NEWS_SENTIMENT,
    /// using the SQLite news cache (1-hour default TTL). Falls back to cached
    /// or empty data on rate-limit / no-key / transport failures so the
    /// surrounding flow never crashes on news. Requires a `Db` previously
    /// attached via [`FinancialDataService::with_db`].
    ///
    /// When a [`NewsInterpreter`] has been attached via
    /// [`with_news_interpreter`](Self::with_news_interpreter), this call
    /// also kicks off a best-effort verdict pass after a successful
    /// fetch. The interpreter short-circuits if the cache row already
    /// has a non-NULL `news_verdict_json`, so no LLM tokens are burned
    /// when the AV cache returns a fresh hit.
    pub async fn fetch_news_sentiment(
        &self,
        symbol: &str,
        lookback_hours: u32,
    ) -> Result<Vec<NewsItem>, Box<dyn Error + Send + Sync>> {
        let db = self
            .db
            .as_ref()
            .ok_or("News fetching requires a Db; call FinancialDataService::with_db first")?;
        let items = news::fetch_news_sentiment_default(
            Arc::clone(db),
            &self.api_key,
            &self.base_url,
            symbol,
            lookback_hours,
        )
        .await?;

        if let Some(interpreter) = self.news_interpreter.as_ref() {
            if let Err(e) = interpreter.interpret(symbol).await {
                warn!("news interpreter failed for {symbol} (best-effort, continuing): {e}");
            }
        }

        Ok(items)
    }

    /// Fetches fundamental data for a given symbol
    pub async fn fetch_fundamental_data(
        &self,
        symbol: &str,
    ) -> Result<crate::ibkr::types::fundamentals::FundamentalData, Box<dyn Error + Send + Sync>>
    {
        let (av_overview, av_income, av_earnings) = tokio::try_join!(
            overview::fetch_overview(
                &self.client,
                &self.api_key,
                &self.base_url,
                &self.cache,
                symbol
            ),
            income::fetch_income_statement(
                &self.client,
                &self.api_key,
                &self.base_url,
                &self.cache,
                symbol
            ),
            earnings::fetch_earnings(
                &self.client,
                &self.api_key,
                &self.base_url,
                &self.cache,
                symbol
            )
        )?;

        let historical = income::process_historical_data(&av_income, &av_earnings);

        if historical.is_empty() {
            return Err(format!(
                "No historical financial data available for {symbol}. This ticker may be too new or not have sufficient financial reporting history."
            ).into());
        }

        let current_metrics = overview::process_current_metrics(&av_overview);
        let analyst_estimates = earnings::process_analyst_estimates(&av_earnings);

        Ok(crate::ibkr::types::fundamentals::FundamentalData {
            symbol: symbol.to_uppercase(),
            historical,
            analyst_estimates,
            current_metrics,
        })
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
