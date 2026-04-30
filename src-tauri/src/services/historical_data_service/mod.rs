//! Historical bar data service.
//!
//! Wraps the IBKR historical-bars endpoint with a SQLite-backed cache,
//! per-key in-flight deduplication, and a 6-req/min sliding-window rate
//! limiter. Designed so the same instance can serve UI queries, the
//! detector framework, and the EOD scheduler from later phases.
//!
//! Production wiring lives in `lib.rs`; tests use a mock fetcher that
//! records calls and returns canned bar batches.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{Duration as ChronoDuration, NaiveDate, TimeZone, Utc};
use tokio::sync::Mutex as TokioMutex;

use crate::ibkr::client::IbkrClient;
use crate::ibkr::error::Result as IbkrResult;
use crate::ibkr::types::historical::{
    parse_ibkr_time, BarSize, HistoricalBar, HistoricalDataRequest, WhatToShow,
};
use crate::middleware::HistoricalRateLimiter;
use crate::storage::Db;

mod cache;

#[cfg(test)]
mod tests;

// ---------------- traits ----------------

/// Narrow trait covering only the historical-data fetch needed by this
/// service. Implemented by the production [`IbkrClient`] and by mock
/// fetchers in tests.
#[async_trait]
pub trait HistoricalDataFetcher: Send + Sync {
    async fn fetch_historical(
        &self,
        request: HistoricalDataRequest,
    ) -> IbkrResult<Vec<HistoricalBar>>;
}

#[async_trait]
impl HistoricalDataFetcher for IbkrClient {
    async fn fetch_historical(
        &self,
        request: HistoricalDataRequest,
    ) -> IbkrResult<Vec<HistoricalBar>> {
        self.get_historical_data(request).await
    }
}

/// Injectable clock so the staleness rules (e.g. "intraday cache only
/// honored same-day") are deterministic in tests.
pub trait Clock: Send + Sync {
    fn today(&self) -> NaiveDate;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn today(&self) -> NaiveDate {
        Utc::now().date_naive()
    }
}

// ---------------- public API ----------------

#[derive(Debug, Clone)]
pub enum Lookback {
    /// `N` calendar days back from "today" (inclusive of today).
    Days(u32),
    /// One specific calendar day in UTC. Used by intraday detectors and
    /// the dedicated single-day cache lookups in tests; production
    /// command surface only exposes `Days(N)` so far.
    #[allow(dead_code)]
    TradingDay(NaiveDate),
}

pub struct HistoricalDataService {
    db: Arc<Db>,
    fetcher: Arc<dyn HistoricalDataFetcher>,
    rate_limit: Arc<HistoricalRateLimiter>,
    clock: Arc<dyn Clock>,
    inflight: Arc<TokioMutex<HashMap<String, Arc<TokioMutex<()>>>>>,
}

impl HistoricalDataService {
    pub fn new(
        db: Arc<Db>,
        fetcher: Arc<dyn HistoricalDataFetcher>,
        rate_limit: Arc<HistoricalRateLimiter>,
    ) -> Self {
        Self {
            db,
            fetcher,
            rate_limit,
            clock: Arc::new(SystemClock),
            inflight: Arc::new(TokioMutex::new(HashMap::new())),
        }
    }

    #[allow(dead_code)]
    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    /// Public entry point. Returns bars for the requested window,
    /// reading from cache when possible and fetching only the missing
    /// portion. Combined output is sorted ascending by `bar_time` and
    /// deduplicated.
    pub async fn fetch_bars(
        &self,
        symbol: &str,
        bar_size: BarSize,
        lookback: Lookback,
    ) -> IbkrResult<Vec<HistoricalBar>> {
        let key = format!(
            "{}|{}|{}",
            symbol,
            bar_size.as_str(),
            lookback_key(&lookback)
        );

        // Take or insert the per-key mutex.
        let per_key = {
            let mut map = self.inflight.lock().await;
            map.entry(key.clone())
                .or_insert_with(|| Arc::new(TokioMutex::new(())))
                .clone()
        };
        let _per_key_guard = per_key.lock().await;

        let result = self.fetch_bars_inner(symbol, bar_size, &lookback).await;

        // Best-effort cleanup so the map doesn't grow without bound.
        // We hold the guard, so strong_count == 2 means: us + the map.
        // Releasing the guard happens at scope end, so check here is fine.
        if Arc::strong_count(&per_key) <= 2 {
            let mut map = self.inflight.lock().await;
            if let Some(existing) = map.get(&key) {
                if Arc::strong_count(existing) <= 2 {
                    map.remove(&key);
                }
            }
        }

        result
    }

    async fn fetch_bars_inner(
        &self,
        symbol: &str,
        bar_size: BarSize,
        lookback: &Lookback,
    ) -> IbkrResult<Vec<HistoricalBar>> {
        let today = self.clock.today();
        let (start_unix, end_unix) = window_bounds(today, lookback);

        // Decide whether the cache is useable for this lookup.
        let cache_usable = !is_cache_stale(bar_size, lookback, today);

        let cached = if cache_usable {
            cache::read_cache(&self.db, symbol, bar_size, start_unix, end_unix).await?
        } else {
            Vec::new()
        };

        let missing = if cache_usable {
            compute_missing_range(&cached, lookback, today)
        } else {
            Some(missing_for_full_window(lookback))
        };

        if let Some(gap_days) = missing {
            self.rate_limit.acquire().await;

            let request = HistoricalDataRequest {
                symbol: symbol.to_string(),
                end_date_time: format!("{} 23:59:59", today.format("%Y%m%d")),
                duration: format!("{gap_days} D"),
                bar_size,
                what_to_show: WhatToShow::Trades,
                use_rth: true,
            };

            let fetched = self.fetcher.fetch_historical(request).await?;
            cache::write_cache(&self.db, symbol, bar_size, &fetched).await?;

            let combined = merge_sorted_unique(cached, fetched);
            return Ok(combined);
        }

        Ok(cached)
    }
}

// ---------------- helpers ----------------

fn lookback_key(lb: &Lookback) -> String {
    match lb {
        Lookback::Days(n) => format!("days:{n}"),
        Lookback::TradingDay(d) => format!("td:{}", d.format("%Y%m%d")),
    }
}

fn day_to_midnight_unix(date: NaiveDate) -> i64 {
    Utc.from_utc_datetime(
        &date
            .and_hms_opt(0, 0, 0)
            .expect("00:00:00 is a valid time on every NaiveDate"),
    )
    .timestamp()
}

fn day_to_eod_unix(date: NaiveDate) -> i64 {
    Utc.from_utc_datetime(
        &date
            .and_hms_opt(23, 59, 59)
            .expect("23:59:59 is a valid time on every NaiveDate"),
    )
    .timestamp()
}

fn window_bounds(today: NaiveDate, lookback: &Lookback) -> (i64, i64) {
    match lookback {
        Lookback::Days(n) => {
            let start = today - ChronoDuration::days(*n as i64);
            (day_to_midnight_unix(start), day_to_eod_unix(today))
        }
        Lookback::TradingDay(d) => (day_to_midnight_unix(*d), day_to_eod_unix(*d)),
    }
}

/// Intraday cache rows are only honored when the lookup is for *today*
/// (system clock). Daily bars never invalidate by age alone.
fn is_cache_stale(bar_size: BarSize, lookback: &Lookback, today: NaiveDate) -> bool {
    if !bar_size.is_intraday() {
        return false;
    }
    match lookback {
        Lookback::TradingDay(d) => *d != today,
        // For an intraday Days(N) lookback we only allow cache to count
        // when N <= 1 and today is the target. This is conservative —
        // good enough for Phase 02; later phases may relax.
        Lookback::Days(n) => *n > 1,
    }
}

/// Returns `Some(gap_days)` when a fetch is needed, `None` if cache covers
/// the request fully. `gap_days` is `>= 1`.
fn compute_missing_range(
    cached: &[HistoricalBar],
    lookback: &Lookback,
    today: NaiveDate,
) -> Option<u32> {
    match lookback {
        Lookback::Days(n) => {
            if cached.is_empty() {
                return Some((*n).max(1));
            }
            let max_cached_ts = cached
                .iter()
                .filter_map(|b| parse_ibkr_time(&b.time).ok())
                .max()
                .unwrap_or(0);
            let max_cached_day = Utc
                .timestamp_opt(max_cached_ts, 0)
                .single()
                .map(|dt| dt.date_naive())
                .unwrap_or(today);
            if max_cached_day >= today {
                // Cache reaches today — for daily bars we treat this as fully covered.
                // Conservative for intraday Days(1), but is_cache_stale would have
                // already pre-empted the intraday-multi-day case.
                return None;
            }
            let gap = (today - max_cached_day).num_days() as u32;
            if gap == 0 {
                None
            } else {
                Some(gap)
            }
        }
        Lookback::TradingDay(_) => {
            if cached.is_empty() {
                Some(1)
            } else {
                None
            }
        }
    }
}

fn missing_for_full_window(lookback: &Lookback) -> u32 {
    match lookback {
        Lookback::Days(n) => (*n).max(1),
        Lookback::TradingDay(_) => 1,
    }
}

/// Merge cached + fetched bar lists, sort ascending by bar_time, dedupe by ts.
fn merge_sorted_unique(a: Vec<HistoricalBar>, b: Vec<HistoricalBar>) -> Vec<HistoricalBar> {
    let mut combined: Vec<HistoricalBar> = a.into_iter().chain(b).collect();
    combined.sort_by(|x, y| {
        let xt = parse_ibkr_time(&x.time).unwrap_or(0);
        let yt = parse_ibkr_time(&y.time).unwrap_or(0);
        xt.cmp(&yt)
    });
    let mut seen: std::collections::HashSet<i64> = std::collections::HashSet::new();
    combined.retain(|bar| {
        let ts = parse_ibkr_time(&bar.time).unwrap_or(0);
        seen.insert(ts)
    });
    combined
}
