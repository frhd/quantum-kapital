// Alpha Vantage fundamentals adapter retained as opportunistic fallback
// (see CompositeFundamentalsProvider). The news path is fully migrated
// to IBKR — see services/news_provider/.

use crate::middleware::AlphaVantageRateLimiter;
use crate::services::cache_service::CacheService;
use async_trait::async_trait;
use reqwest::Client;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::error::Error;
use std::path::PathBuf;
use std::sync::{Arc, Mutex as StdMutex};
use thiserror::Error;
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

mod earnings;
mod income;
mod overview;

#[cfg(test)]
mod fundamentals_tests;

/// HTTP transport seam for the Alpha Vantage fundamentals endpoints.
/// Mirrors the news-side [`news::NewsHttp`] trait so tests can return
/// canned JSON without standing up a real HTTP server.
#[async_trait]
pub trait AvHttp: Send + Sync {
    async fn fetch(&self, url: &str) -> Result<Value, AvHttpError>;
}

#[derive(Error, Debug)]
pub enum AvHttpError {
    #[error("transport: {0}")]
    Transport(String),
    #[error("status: {0}")]
    Status(String),
}

pub struct ReqwestAvHttp {
    client: Client,
}

impl ReqwestAvHttp {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
        }
    }
}

impl Default for ReqwestAvHttp {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AvHttp for ReqwestAvHttp {
    async fn fetch(&self, url: &str) -> Result<Value, AvHttpError> {
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| AvHttpError::Transport(e.to_string()))?;
        if !response.status().is_success() {
            return Err(AvHttpError::Status(response.status().to_string()));
        }
        response
            .json::<Value>()
            .await
            .map_err(|e| AvHttpError::Transport(e.to_string()))
    }
}

/// Result type stored in the in-flight coalescing map. Errors are
/// stringified so the result can be `Clone`'d and shared with multiple
/// awaiting callers; the public boxed-error API is reconstructed in
/// `fetch_fundamental_data`.
type FundamentalsResult = Result<crate::ibkr::types::fundamentals::FundamentalData, String>;

enum FetchSlot {
    /// Joining an in-flight leader: subscribe and wait for their result.
    Joined(broadcast::Receiver<Arc<FundamentalsResult>>),
    /// Claimed leadership: leader runs the work, then broadcasts.
    Leader(broadcast::Sender<Arc<FundamentalsResult>>),
}

/// Stale fundamentals older than this trigger a louder warning when
/// served from the stale-cache fallback path. Day-stale is acceptable
/// when AV is rate-limited; year-stale is a smell.
const STALE_FUNDAMENTALS_WARN_AGE_SECS: u64 = 30 * 24 * 60 * 60;

/// Cache-key suffixes for the three AV fundamentals endpoints. Shared
/// between the writers (`overview::fetch_overview`, etc.) and the
/// invalidators (`clear_fundamentals_cache`, the composite provider's
/// stale-read helper). Centralising this list is the only thing that
/// keeps Hard Invariant #8 ("manual write invalidates AV cache")
/// honest — a divergence between the writer suffix and the clearer
/// suffix silently leaks pre-manual data through the cache.
pub const AV_FUNDAMENTALS_CACHE_SUFFIXES: [&str; 3] = ["overview", "income_statement", "earnings"];

/// Service for fetching fundamental data from Alpha Vantage API
pub struct FinancialDataService {
    av_http: Arc<dyn AvHttp>,
    api_key: String,
    base_url: String,
    cache: Option<CacheService>,
    rate_limiter: Option<Arc<AlphaVantageRateLimiter>>,
    /// Per-symbol coalescing map. While a fetch for `SYMBOL` is in
    /// flight, additional callers subscribe to the broadcast and wait
    /// for the leader's result instead of issuing more AV requests.
    /// The slot is cleared on completion (success or error), so a
    /// failed fetch never poisons future attempts.
    inflight_fundamentals:
        Arc<StdMutex<HashMap<String, broadcast::Sender<Arc<FundamentalsResult>>>>>,
}

#[derive(Debug)]
enum AvCheck {
    Hard(String),
    SoftSkip(String),
}

/// Classify an Alpha Vantage JSON payload. `Hard` errors propagate
/// immediately; `SoftSkip` (rate-limit `Note` or quota `Information`)
/// triggers the stale-cache fallback in `fetch_av_function`.
fn classify_av_response(json: &Value) -> Result<(), AvCheck> {
    if let Some(error_msg) = json.get("Error Message").and_then(|v| v.as_str()) {
        return Err(AvCheck::Hard(format!(
            "Alpha Vantage API error: {error_msg}"
        )));
    }
    if let Some(note) = json.get("Note").and_then(|v| v.as_str()) {
        return Err(AvCheck::SoftSkip(note.to_string()));
    }
    if let Some(info) = json.get("Information").and_then(|v| v.as_str()) {
        return Err(AvCheck::SoftSkip(info.to_string()));
    }
    Ok(())
}

/// Fetch a single Alpha Vantage `function=...` payload, hitting the local
/// JSON cache first and falling back to the live API. Cache key is namespaced
/// per (symbol, suffix) so the three callers — OVERVIEW, INCOME_STATEMENT,
/// EARNINGS — don't collide on disk.
///
/// On rate-limit / quota soft-skip responses, falls back to the most
/// recently cached payload (even if past TTL) and emits a `warn!`. This
/// mirrors the news-path behavior in `news.rs` and keeps the surrounding
/// agent loop alive when AV says "you're done for the day."
#[allow(clippy::too_many_arguments)]
pub(super) async fn fetch_av_function<T>(
    http: &dyn AvHttp,
    rate_limiter: Option<&AlphaVantageRateLimiter>,
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

    if let Some(limiter) = rate_limiter {
        limiter.acquire().await;
    }

    let json = match http.fetch(&url).await {
        Ok(v) => v,
        Err(e) => {
            warn!("Alpha Vantage {function} HTTP fetch failed for {symbol}: {e}");
            if let Some(value) = read_stale_cache::<T>(cache, &cache_key, function, symbol) {
                return Ok(value);
            }
            return Err(Box::new(e));
        }
    };

    match classify_av_response(&json) {
        Ok(()) => {
            let parsed: T = serde_json::from_value(json)?;
            if let Some(ref c) = cache {
                let _ = c.write(&cache_key, &parsed);
            }
            Ok(parsed)
        }
        Err(AvCheck::SoftSkip(msg)) => {
            warn!("Alpha Vantage {function} soft-skip for {symbol}: {msg}");
            if let Some(value) = read_stale_cache::<T>(cache, &cache_key, function, symbol) {
                return Ok(value);
            }
            Err(format!("Alpha Vantage {function}: {msg}").into())
        }
        Err(AvCheck::Hard(msg)) => Err(msg.into()),
    }
}

/// Read a stale-allowed cache entry for the given key. Returns `None`
/// when no cache row exists, or when the cache hasn't been provisioned.
/// Logs at `warn!` on hit so operators can see the fallback firing; the
/// log gets louder when the entry is older than 30 days (don't silently
/// serve year-old fundamentals).
fn read_stale_cache<T>(
    cache: &Option<CacheService>,
    cache_key: &str,
    function: &str,
    symbol: &str,
) -> Option<T>
where
    T: DeserializeOwned,
{
    let c = cache.as_ref()?;
    match c.read_ignoring_ttl::<T>(cache_key) {
        Ok((value, age)) => {
            if age >= STALE_FUNDAMENTALS_WARN_AGE_SECS {
                warn!(
                    "serving very stale cached {function} data for {symbol} \
                     (age {}s, > {} day threshold) — refresh next time AV is reachable",
                    age,
                    STALE_FUNDAMENTALS_WARN_AGE_SECS / 86_400
                );
            } else {
                warn!(
                    "serving stale cached {function} data for {symbol} (age {}s)",
                    age
                );
            }
            Some(value)
        }
        Err(_) => None,
    }
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
            av_http: Arc::new(ReqwestAvHttp::new()),
            api_key,
            base_url: "https://www.alphavantage.co/query".to_string(),
            cache,
            rate_limiter: None,
            inflight_fundamentals: Arc::new(StdMutex::new(HashMap::new())),
        }
    }

    /// Attach a per-vendor rate limiter. AV's free tier permits 1
    /// request per second across the entire account; without this,
    /// `tokio::try_join!`-style parallel fetches burn the per-second
    /// quota in a single tick.
    pub fn with_rate_limiter(mut self, limiter: Arc<AlphaVantageRateLimiter>) -> Self {
        self.rate_limiter = Some(limiter);
        self
    }

    /// Override the HTTP transport. Production wires `ReqwestAvHttp`;
    /// tests inject a counter-fake to assert request counts (e.g., for
    /// the in-flight coalescing test).
    #[cfg(test)]
    pub(crate) fn with_http(mut self, http: Arc<dyn AvHttp>) -> Self {
        self.av_http = http;
        self
    }

    /// Inject a custom `CacheService` (e.g., a `with_ttl` variant for
    /// the stale-cache-fallback test).
    #[cfg(test)]
    pub(crate) fn with_cache(mut self, cache: CacheService) -> Self {
        self.cache = Some(cache);
        self
    }

    /// Test-only accessor for the inner cache. Used by Phase 4 tests
    /// that need to assert cache rows are present/cleared without
    /// re-implementing the file-on-disk inspection.
    #[cfg(test)]
    pub(crate) fn cache_for_test(&self) -> &CacheService {
        self.cache.as_ref().expect("cache wired in test")
    }

    /// Drop every cached AV fundamentals row for `symbol`. Phase 4: the
    /// MCP `set_fundamentals` tool calls this after a manual write so the
    /// AV file cache cannot resurface a pre-manual payload if the manual
    /// row is later cleared. Each AV fundamentals fetch hits three
    /// endpoints (`OVERVIEW` / `INCOME_STATEMENT` / `EARNINGS`), keyed
    /// `<SYMBOL>_overview` / `_income` / `_earnings` — purge all three.
    /// Errors are logged and swallowed individually so a partial purge
    /// (one of the three keys absent) does not fail the surrounding write.
    pub fn clear_fundamentals_cache(&self, symbol: &str) {
        let Some(cache) = self.cache.as_ref() else {
            return;
        };
        let upper = symbol.trim().to_uppercase();
        if upper.is_empty() {
            return;
        }
        for suffix in AV_FUNDAMENTALS_CACHE_SUFFIXES {
            let key = format!("{upper}_{suffix}");
            if let Err(e) = cache.clear(&key) {
                warn!("AV cache clear failed for {key}: {e}");
            }
        }
    }

    /// Reconstruct a [`FundamentalData`] from the AV file cache for
    /// `symbol`, allowing TTL-expired entries. Returns `None` if any
    /// of the three endpoint rows is missing or unparseable. Phase 5
    /// uses this to serve stale data on AV-cap exhaustion without
    /// hitting the wire (the AV adapter's per-endpoint stale fallback
    /// only fires on transport / rate-limit errors, not on a
    /// composite-side budget refusal).
    pub fn read_cached_fundamentals_ignoring_ttl(
        cache: &CacheService,
        symbol: &str,
    ) -> Option<crate::ibkr::types::fundamentals::FundamentalData> {
        let upper = symbol.trim().to_uppercase();
        if upper.is_empty() {
            return None;
        }
        let overview: overview::AlphaVantageOverview = cache
            .read_ignoring_ttl(&format!("{upper}_overview"))
            .ok()
            .map(|(v, _age)| v)?;
        let income: income::AlphaVantageIncomeStatement = cache
            .read_ignoring_ttl(&format!("{upper}_income_statement"))
            .ok()
            .map(|(v, _age)| v)?;
        let earnings: earnings::AlphaVantageEarnings = cache
            .read_ignoring_ttl(&format!("{upper}_earnings"))
            .ok()
            .map(|(v, _age)| v)?;
        let historical = income::process_historical_data(&income, &earnings);
        if historical.is_empty() {
            return None;
        }
        Some(crate::ibkr::types::fundamentals::FundamentalData {
            symbol: upper,
            historical,
            analyst_estimates: earnings::process_analyst_estimates(&earnings),
            current_metrics: overview::process_current_metrics(&overview),
        })
    }

    /// Fetches fundamental data for a given symbol.
    ///
    /// Concurrent calls for the same symbol coalesce: the first call
    /// fans out to AV (3 endpoints) while later callers join the
    /// in-flight broadcast and reuse the result. The slot is cleared on
    /// completion so a failed fetch never poisons future attempts.
    pub async fn fetch_fundamental_data(
        &self,
        symbol: &str,
    ) -> Result<crate::ibkr::types::fundamentals::FundamentalData, Box<dyn Error + Send + Sync>>
    {
        let key = symbol.to_uppercase();

        let leader_tx = match self.claim_or_join(&key) {
            FetchSlot::Joined(mut rx) => {
                debug!("Coalescing fundamentals fetch for {symbol}");
                return match rx.recv().await {
                    Ok(arc) => match arc.as_ref() {
                        Ok(d) => Ok(d.clone()),
                        Err(s) => Err(s.clone().into()),
                    },
                    Err(e) => Err(format!(
                        "Coalesced fundamentals fetch for {symbol} dropped before completion: {e}"
                    )
                    .into()),
                };
            }
            FetchSlot::Leader(tx) => tx,
        };

        // We are the leader. Run the actual fan-out fetch.
        let result = self.do_fetch_fundamental_data(symbol).await;
        let stringified: FundamentalsResult = match &result {
            Ok(d) => Ok(d.clone()),
            Err(e) => Err(e.to_string()),
        };

        // Clear the slot BEFORE broadcasting so any caller that arrives
        // after this point starts a fresh fetch.
        self.release_slot(&key);
        let _ = leader_tx.send(Arc::new(stringified));

        result
    }

    /// Sync helper: under the inflight mutex, either subscribe to an
    /// existing leader's broadcast or claim leadership by inserting a
    /// fresh sender. Kept synchronous so the `MutexGuard` never crosses
    /// an `.await` (would otherwise make the enclosing future `!Send`).
    fn claim_or_join(&self, key: &str) -> FetchSlot {
        let mut map = self
            .inflight_fundamentals
            .lock()
            .expect("inflight_fundamentals mutex poisoned");
        if let Some(tx) = map.get(key) {
            return FetchSlot::Joined(tx.subscribe());
        }
        let (tx, _rx0) = broadcast::channel::<Arc<FundamentalsResult>>(1);
        map.insert(key.to_string(), tx.clone());
        FetchSlot::Leader(tx)
    }

    fn release_slot(&self, key: &str) {
        let mut map = self
            .inflight_fundamentals
            .lock()
            .expect("inflight_fundamentals mutex poisoned");
        map.remove(key);
    }

    /// Underlying fan-out fetch: fires OVERVIEW + INCOME_STATEMENT +
    /// EARNINGS in parallel. Each call honours the rate limiter, so
    /// `try_join!` issues them on the wire in 1-req-per-second order
    /// even though the futures themselves run concurrently.
    async fn do_fetch_fundamental_data(
        &self,
        symbol: &str,
    ) -> Result<crate::ibkr::types::fundamentals::FundamentalData, Box<dyn Error + Send + Sync>>
    {
        let limiter_ref = self.rate_limiter.as_deref();
        let http_ref: &dyn AvHttp = self.av_http.as_ref();
        let (av_overview, av_income, av_earnings) = tokio::try_join!(
            overview::fetch_overview(
                http_ref,
                limiter_ref,
                &self.api_key,
                &self.base_url,
                &self.cache,
                symbol
            ),
            income::fetch_income_statement(
                http_ref,
                limiter_ref,
                &self.api_key,
                &self.base_url,
                &self.cache,
                symbol
            ),
            earnings::fetch_earnings(
                http_ref,
                limiter_ref,
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
