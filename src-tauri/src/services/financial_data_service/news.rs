//! Phase 03 — Alpha Vantage NEWS_SENTIMENT fetcher with SQLite-backed cache.
//!
//! Used by Phase 08 (EP detector) and Phase 19 (LLM news interpreter). The
//! service is best-effort: rate-limited or transport failures fall back to
//! the most recently cached payload (or an empty list if none exists), so
//! a missing API key never crashes the surrounding flow.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use thiserror::Error;
use tracing::{debug, warn};

use crate::ibkr::types::news::{NewsItem, TickerSentiment};
use crate::middleware::AlphaVantageRateLimiter;
use crate::storage::Db;

/// Default cache TTL for news. Callers that need stricter freshness can
/// pass a smaller value into [`fetch_news_sentiment_with_deps`].
pub const DEFAULT_NEWS_TTL_SECS: i64 = 60 * 60;

/// HTTP transport seam, kept narrow so tests can return canned JSON
/// without standing up a real HTTP server. Production wires the
/// reqwest-backed [`ReqwestNewsHttp`].
#[async_trait]
pub trait NewsHttp: Send + Sync {
    async fn fetch(&self, url: &str) -> Result<Value, NewsHttpError>;
}

#[derive(Error, Debug)]
pub enum NewsHttpError {
    #[error("transport: {0}")]
    Transport(String),
    #[error("status: {0}")]
    Status(String),
}

/// Injectable clock — same idea as `services::historical_data_service::Clock`,
/// but with second-resolution semantics since the news cache TTL is in
/// seconds.
pub trait NewsClock: Send + Sync {
    fn now_unix(&self) -> i64;
}

pub struct SystemNewsClock;

impl NewsClock for SystemNewsClock {
    fn now_unix(&self) -> i64 {
        chrono::Utc::now().timestamp()
    }
}

pub struct ReqwestNewsHttp {
    pub client: reqwest::Client,
}

impl ReqwestNewsHttp {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }
}

impl Default for ReqwestNewsHttp {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl NewsHttp for ReqwestNewsHttp {
    async fn fetch(&self, url: &str) -> Result<Value, NewsHttpError> {
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| NewsHttpError::Transport(e.to_string()))?;
        if !response.status().is_success() {
            return Err(NewsHttpError::Status(response.status().to_string()));
        }
        response
            .json::<Value>()
            .await
            .map_err(|e| NewsHttpError::Transport(e.to_string()))
    }
}

/// Parse an Alpha Vantage NEWS_SENTIMENT response, filtering items that
/// reference `requested_symbol` in their `ticker_sentiment` array. Items
/// with malformed top-level fields are dropped; missing optional fields
/// (e.g. `overall_sentiment_score`) decode to `None`.
pub fn parse_news_response(json: &Value, requested_symbol: &str) -> Vec<NewsItem> {
    let symbol = requested_symbol.to_uppercase();
    let Some(feed) = json.get("feed").and_then(|v| v.as_array()) else {
        return Vec::new();
    };

    feed.iter()
        .filter_map(parse_news_item)
        .filter(|item| {
            item.ticker_sentiment
                .iter()
                .any(|ts| ts.ticker.eq_ignore_ascii_case(&symbol))
        })
        .collect()
}

fn parse_news_item(raw: &Value) -> Option<NewsItem> {
    let time_str = raw.get("time_published")?.as_str()?;
    let time_published = parse_av_time(time_str)?;
    let title = raw.get("title")?.as_str()?.to_string();
    let summary = raw
        .get("summary")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let source = raw
        .get("source")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let url = raw
        .get("url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // null → None; missing → None; numeric → Some
    let overall_sentiment_score = match raw.get("overall_sentiment_score") {
        Some(Value::Null) | None => None,
        Some(v) => v.as_f64(),
    };
    let overall_sentiment_label = match raw.get("overall_sentiment_label") {
        Some(Value::String(s)) => Some(s.clone()),
        _ => None,
    };
    let ticker_sentiment = raw
        .get("ticker_sentiment")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(parse_ticker_sentiment).collect())
        .unwrap_or_default();

    Some(NewsItem {
        time_published,
        title,
        summary,
        source,
        url,
        overall_sentiment_score,
        overall_sentiment_label,
        ticker_sentiment,
    })
}

fn parse_ticker_sentiment(raw: &Value) -> Option<TickerSentiment> {
    let ticker = raw.get("ticker")?.as_str()?.to_string();
    let relevance_score = parse_string_or_number(raw.get("relevance_score")).unwrap_or(0.0);
    let ticker_sentiment_score =
        parse_string_or_number(raw.get("ticker_sentiment_score")).unwrap_or(0.0);
    let ticker_sentiment_label = raw
        .get("ticker_sentiment_label")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    Some(TickerSentiment {
        ticker,
        relevance_score,
        ticker_sentiment_score,
        ticker_sentiment_label,
    })
}

fn parse_string_or_number(v: Option<&Value>) -> Option<f64> {
    match v? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse::<f64>().ok(),
        _ => None,
    }
}

fn parse_av_time(s: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    use chrono::NaiveDateTime;
    NaiveDateTime::parse_from_str(s.trim(), "%Y%m%dT%H%M%S")
        .ok()
        .map(|nd| nd.and_utc())
}

#[derive(Debug)]
enum AvError {
    Hard(String),
    SoftSkip(String),
}

fn classify_av_response(json: &Value) -> Result<(), AvError> {
    if let Some(error_msg) = json.get("Error Message").and_then(|v| v.as_str()) {
        return Err(AvError::Hard(format!(
            "Alpha Vantage API error: {error_msg}"
        )));
    }
    if let Some(note) = json.get("Note").and_then(|v| v.as_str()) {
        return Err(AvError::SoftSkip(note.to_string()));
    }
    if let Some(info) = json.get("Information").and_then(|v| v.as_str()) {
        return Err(AvError::SoftSkip(info.to_string()));
    }
    Ok(())
}

/// Read-through cache + fetch with rate-limit / no-key fallbacks.
///
/// - If the cache row is younger than `ttl_secs`, return it without calling
///   `http`.
/// - Otherwise call `http`. On HTTP success and a clean payload, parse,
///   write-through to cache, return parsed.
/// - On rate-limit / `Note` / `Information` responses, log a warn and fall
///   back to the cached payload (even if stale). If no cached payload
///   exists, return `Ok(vec![])` — news is best-effort.
/// - On transport-level failures, same fallback path as rate-limit.
/// - An empty `api_key` triggers the no-key path: skip the HTTP call,
///   return cached or empty.
#[allow(clippy::too_many_arguments)]
pub async fn fetch_news_sentiment_with_deps<H, C>(
    http: &H,
    clock: &C,
    db: &Db,
    rate_limiter: Option<&AlphaVantageRateLimiter>,
    api_key: &str,
    base_url: &str,
    symbol: &str,
    _lookback_hours: u32,
    ttl_secs: i64,
) -> Result<Vec<NewsItem>, Box<dyn std::error::Error + Send + Sync>>
where
    H: NewsHttp + ?Sized,
    C: NewsClock + ?Sized,
{
    let symbol_upper = symbol.to_uppercase();
    let now = clock.now_unix();

    let cached = read_cache(db, &symbol_upper).await?;

    // Fresh-cache fast path.
    if let Some((fetched_at, items)) = &cached {
        if now.saturating_sub(*fetched_at) <= ttl_secs {
            debug!(
                "news cache hit for {} (age {}s)",
                symbol_upper,
                now - fetched_at
            );
            return Ok(items.clone());
        }
    }

    // No-key fallback: skip HTTP entirely.
    if api_key.trim().is_empty() {
        warn!(
            "ALPHA_VANTAGE_API_KEY not set; returning {} for {}",
            if cached.is_some() {
                "cached news"
            } else {
                "empty news"
            },
            symbol_upper
        );
        return Ok(cached.map(|(_, items)| items).unwrap_or_default());
    }

    let url = format!(
        "{base_url}?function=NEWS_SENTIMENT&tickers={symbol_upper}&limit=50&apikey={api_key}"
    );

    if let Some(limiter) = rate_limiter {
        limiter.acquire().await;
    }

    match http.fetch(&url).await {
        Ok(json) => match classify_av_response(&json) {
            Ok(()) => {
                let parsed = parse_news_response(&json, &symbol_upper);
                write_cache(db, &symbol_upper, now, &parsed).await?;
                Ok(parsed)
            }
            Err(AvError::SoftSkip(msg)) => {
                warn!(
                    "Alpha Vantage news soft-skip for {symbol_upper}: {msg}; \
                     falling back to cache"
                );
                Ok(cached.map(|(_, items)| items).unwrap_or_default())
            }
            Err(AvError::Hard(msg)) => Err(msg.into()),
        },
        Err(e) => {
            warn!("news HTTP fetch failed for {symbol_upper}: {e}; falling back to cache");
            Ok(cached.map(|(_, items)| items).unwrap_or_default())
        }
    }
}

async fn read_cache(
    db: &Db,
    symbol: &str,
) -> Result<Option<(i64, Vec<NewsItem>)>, Box<dyn std::error::Error + Send + Sync>> {
    let symbol = symbol.to_string();
    let row = db
        .with_conn(move |conn| {
            let mut stmt =
                conn.prepare("SELECT fetched_at, payload FROM news_cache WHERE symbol = ?1")?;
            let mut rows = stmt.query(rusqlite::params![symbol])?;
            if let Some(row) = rows.next()? {
                let fetched_at: i64 = row.get(0)?;
                let payload: String = row.get(1)?;
                Ok(Some((fetched_at, payload)))
            } else {
                Ok(None)
            }
        })
        .await?;

    match row {
        Some((fetched_at, payload)) => {
            let items: Vec<NewsItem> = serde_json::from_str(&payload)?;
            Ok(Some((fetched_at, items)))
        }
        None => Ok(None),
    }
}

/// Read the cached news payload + LLM verdict for `symbol`. Returns
/// `Ok(None)` when no row exists. The verdict column is `NULL` until
/// the news interpreter populates it.
pub async fn read_cache_with_verdict(
    db: &Db,
    symbol: &str,
) -> Result<Option<CachedNews>, Box<dyn std::error::Error + Send + Sync>> {
    let symbol = symbol.to_string();
    let row = db
        .with_conn(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT fetched_at, payload, news_verdict_json FROM news_cache WHERE symbol = ?1",
            )?;
            let mut rows = stmt.query(rusqlite::params![symbol])?;
            if let Some(row) = rows.next()? {
                let fetched_at: i64 = row.get(0)?;
                let payload: String = row.get(1)?;
                let verdict_json: Option<String> = row.get(2)?;
                Ok(Some((fetched_at, payload, verdict_json)))
            } else {
                Ok(None)
            }
        })
        .await?;

    match row {
        Some((fetched_at, payload, verdict_json)) => {
            let items: Vec<NewsItem> = serde_json::from_str(&payload)?;
            Ok(Some(CachedNews {
                fetched_at,
                items,
                verdict_json,
            }))
        }
        None => Ok(None),
    }
}

/// Cached news row, including the optional LLM-derived verdict JSON.
#[derive(Debug, Clone)]
pub struct CachedNews {
    /// Unix seconds at which the AV payload landed in cache. Carried
    /// along the read path for callers that want to age the verdict.
    #[allow(dead_code)]
    pub fetched_at: i64,
    pub items: Vec<NewsItem>,
    /// Raw JSON of [`crate::ibkr::types::NewsVerdict`]. `None` when the
    /// interpreter has not yet run for the current payload (every fresh
    /// `write_cache` clears the column).
    pub verdict_json: Option<String>,
}

/// Persist the news interpreter verdict for `symbol`. No-ops cleanly
/// when no `news_cache` row exists (the interpreter only runs after
/// the fetcher has written one). `verdict_json` is stored verbatim.
pub async fn write_verdict(
    db: &Db,
    symbol: &str,
    verdict_json: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let symbol = symbol.to_string();
    let verdict_json = verdict_json.to_string();
    db.with_conn(move |conn| {
        conn.execute(
            "UPDATE news_cache SET news_verdict_json = ?2 WHERE symbol = ?1",
            rusqlite::params![symbol, verdict_json],
        )?;
        Ok(())
    })
    .await?;
    Ok(())
}

async fn write_cache(
    db: &Db,
    symbol: &str,
    fetched_at: i64,
    items: &[NewsItem],
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let symbol = symbol.to_string();
    let payload = serde_json::to_string(items)?;
    // INSERT OR REPLACE drops the existing row, which clears the
    // `news_verdict_json` column. That's intentional: a new payload
    // invalidates any prior verdict and the interpreter must re-run.
    db.with_conn(move |conn| {
        conn.execute(
            "INSERT OR REPLACE INTO news_cache (symbol, fetched_at, payload) \
             VALUES (?1, ?2, ?3)",
            rusqlite::params![symbol, fetched_at, payload],
        )?;
        Ok(())
    })
    .await?;
    Ok(())
}

/// Convenience used by the production [`super::FinancialDataService`] —
/// instantiates the reqwest transport and system clock and delegates.
pub async fn fetch_news_sentiment_default(
    db: Arc<Db>,
    rate_limiter: Option<&AlphaVantageRateLimiter>,
    api_key: &str,
    base_url: &str,
    symbol: &str,
    lookback_hours: u32,
) -> Result<Vec<NewsItem>, Box<dyn std::error::Error + Send + Sync>> {
    let http = ReqwestNewsHttp::new();
    let clock = SystemNewsClock;
    fetch_news_sentiment_with_deps(
        &http,
        &clock,
        &db,
        rate_limiter,
        api_key,
        base_url,
        symbol,
        lookback_hours,
        DEFAULT_NEWS_TTL_SECS,
    )
    .await
}
