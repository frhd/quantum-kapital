//! Fixture-backed fakes for testing [`super::IbkrNewsProvider`]
//! without a live TWS connection. The Phase 6 spike captured three
//! payloads under `tests/fixtures/ibkr_news/` and we replay them here
//! verbatim — `include_str!` keeps the fixture files coupled to the
//! parser tests at compile time.

#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Deserialize;

use super::super::NewsError;
use super::client::{IbkrHeadline, IbkrNewsClient, IbkrNewsProviderInfo};

const PHASE6_NEWS_PROVIDERS_JSON: &str =
    include_str!("../../../../tests/fixtures/ibkr_news/news_providers.json");
const PHASE6_AAPL_HISTORICAL_JSON: &str =
    include_str!("../../../../tests/fixtures/ibkr_news/AAPL_historical.json");

#[derive(Debug, Deserialize)]
struct WireProviderRow {
    code: String,
    name: String,
}

#[derive(Debug, Deserialize)]
struct WireHistoricalRow {
    time_iso8601: String,
    provider_code: String,
    article_id: String,
    headline: String,
    #[serde(default)]
    extra_data: String,
}

/// Parse the Phase 6 `news_providers.json` fixture into the trait
/// domain shape. Panics on malformed JSON — the fixture is checked
/// into the repo, so a bad payload here is a regression in Phase 6's
/// capture, not a runtime concern.
pub fn phase6_news_providers() -> Vec<IbkrNewsProviderInfo> {
    let rows: Vec<WireProviderRow> =
        serde_json::from_str(PHASE6_NEWS_PROVIDERS_JSON).expect("phase 6 news_providers.json");
    rows.into_iter()
        .map(|r| IbkrNewsProviderInfo {
            code: r.code,
            name: r.name,
        })
        .collect()
}

/// Parse the Phase 6 `AAPL_historical.json` fixture into [`IbkrHeadline`]s.
pub fn phase6_aapl_headlines() -> Vec<IbkrHeadline> {
    let rows: Vec<WireHistoricalRow> =
        serde_json::from_str(PHASE6_AAPL_HISTORICAL_JSON).expect("phase 6 AAPL_historical.json");
    rows.into_iter()
        .map(|r| IbkrHeadline {
            time: parse_rfc3339(&r.time_iso8601),
            provider_code: r.provider_code,
            article_id: r.article_id,
            headline: r.headline,
            extra_data: r.extra_data,
        })
        .collect()
}

fn parse_rfc3339(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s)
        .unwrap_or_else(|e| panic!("phase 6 fixture time {s:?} not RFC3339: {e}"))
        .with_timezone(&Utc)
}

/// Programmable [`IbkrNewsClient`] for tests — pre-loaded from the
/// Phase 6 fixtures by [`Self::with_phase6_aapl`], or constructed
/// blank with [`Self::new`] and seeded explicitly for edge-case
/// tests (subscription denial, rate-limit, etc.).
pub struct FixtureIbkrNewsClient {
    providers: Mutex<Vec<IbkrNewsProviderInfo>>,
    /// Symbol-keyed canned headline list. Lookup is uppercased so
    /// callers can pass either case. Symbols not present return an
    /// empty `Vec<IbkrHeadline>` (the `Ok(vec![])` no-news contract).
    headlines: Mutex<HashMap<String, Vec<IbkrHeadline>>>,
    forced_provider_error: Mutex<Option<NewsError>>,
    forced_historical_error: Mutex<Option<NewsError>>,
}

impl FixtureIbkrNewsClient {
    pub fn new() -> Self {
        Self {
            providers: Mutex::new(Vec::new()),
            headlines: Mutex::new(HashMap::new()),
            forced_provider_error: Mutex::new(None),
            forced_historical_error: Mutex::new(None),
        }
    }

    /// Pre-load the Phase 6 AAPL fixtures (the 8-provider directory +
    /// 50-headline AAPL list). Most fixture-driven tests want this.
    pub fn with_phase6_aapl() -> Self {
        let me = Self::new();
        me.set_providers(phase6_news_providers());
        me.set_headlines("AAPL", phase6_aapl_headlines());
        me
    }

    pub fn set_providers(&self, providers: Vec<IbkrNewsProviderInfo>) {
        *self.providers.lock().unwrap() = providers;
    }

    pub fn set_headlines(&self, symbol: impl Into<String>, headlines: Vec<IbkrHeadline>) {
        let key = symbol.into().to_uppercase();
        self.headlines.lock().unwrap().insert(key, headlines);
    }

    /// Force every subsequent `news_providers()` call to surface this
    /// error. Use [`NewsError::NotConnected`] / [`NewsError::Other`]
    /// to model the TWS-down path.
    pub fn fail_news_providers(&self, err: NewsError) {
        *self.forced_provider_error.lock().unwrap() = Some(err);
    }

    /// Force every subsequent `historical_news()` call to surface
    /// this error — used by the subscription-denied test path.
    pub fn fail_historical_news(&self, err: NewsError) {
        *self.forced_historical_error.lock().unwrap() = Some(err);
    }
}

impl Default for FixtureIbkrNewsClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl IbkrNewsClient for FixtureIbkrNewsClient {
    async fn news_providers(&self) -> Result<Vec<IbkrNewsProviderInfo>, NewsError> {
        if let Some(err) = self.forced_provider_error.lock().unwrap().as_ref() {
            return Err(clone_news_error(err));
        }
        Ok(self.providers.lock().unwrap().clone())
    }

    async fn historical_news(
        &self,
        symbol: &str,
        _provider_codes: &[String],
        _lookback_hours: u32,
        total_results: u8,
    ) -> Result<Vec<IbkrHeadline>, NewsError> {
        if let Some(err) = self.forced_historical_error.lock().unwrap().as_ref() {
            return Err(clone_news_error(err));
        }
        let key = symbol.trim().to_uppercase();
        let rows = self
            .headlines
            .lock()
            .unwrap()
            .get(&key)
            .cloned()
            .unwrap_or_default();
        Ok(rows.into_iter().take(total_results as usize).collect())
    }
}

fn clone_news_error(err: &NewsError) -> NewsError {
    use std::time::Duration;
    match err {
        NewsError::RateLimited { retry_after } => NewsError::RateLimited {
            retry_after: retry_after.map(Duration::from),
        },
        NewsError::NoSubscription { provider_code } => NewsError::NoSubscription {
            provider_code: provider_code.clone(),
        },
        NewsError::NotConnected => NewsError::NotConnected,
        NewsError::ParseError(s) => NewsError::ParseError(s.clone()),
        NewsError::Other(s) => NewsError::Other(s.clone()),
    }
}
