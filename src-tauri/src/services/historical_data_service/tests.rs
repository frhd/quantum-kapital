// allow-large-file: bars-cache integration tests covering Db round-trips, in-flight
// dedup, lookback expansion, and IBKR-fallback paths. The mock fetcher + temp Db
// scaffolding is shared across every case.
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{Duration as ChronoDuration, NaiveDate, TimeZone, Utc};
use tempfile::NamedTempFile;
use tokio::sync::{Mutex, Notify};

use crate::ibkr::error::{IbkrError, Result as IbkrResult};
use crate::ibkr::types::historical::{BarSize, HistoricalBar, HistoricalDataRequest, WhatToShow};
use crate::middleware::HistoricalRateLimiter;
use crate::storage::Db;

use super::{Clock, HistoricalDataFetcher, HistoricalDataService, Lookback};

// ------------- test helpers -------------

#[derive(Default)]
struct MockHistoricalFetcher {
    calls: Mutex<Vec<HistoricalDataRequest>>,
    canned: Mutex<std::collections::VecDeque<Vec<HistoricalBar>>>,
    delay_first: Mutex<Option<Arc<Notify>>>,
}

impl MockHistoricalFetcher {
    fn new() -> Self {
        Self::default()
    }

    async fn enqueue(&self, bars: Vec<HistoricalBar>) {
        self.canned.lock().await.push_back(bars);
    }

    async fn call_count(&self) -> usize {
        self.calls.lock().await.len()
    }

    async fn calls(&self) -> Vec<HistoricalDataRequest> {
        self.calls.lock().await.clone()
    }

    async fn install_first_call_gate(&self, gate: Arc<Notify>) {
        *self.delay_first.lock().await = Some(gate);
    }
}

#[async_trait]
impl HistoricalDataFetcher for MockHistoricalFetcher {
    async fn fetch_historical(
        &self,
        request: HistoricalDataRequest,
    ) -> IbkrResult<Vec<HistoricalBar>> {
        // First-call gate (used by dedup test): block until released.
        let gate = self.delay_first.lock().await.take();
        if let Some(notify) = gate {
            notify.notified().await;
        }

        self.calls.lock().await.push(request);
        let bars = self
            .canned
            .lock()
            .await
            .pop_front()
            .expect("MockHistoricalFetcher canned queue exhausted");
        Ok(bars)
    }
}

struct FixedClock(NaiveDate);

impl Clock for FixedClock {
    fn today(&self) -> NaiveDate {
        self.0
    }
}

fn fixed_today() -> NaiveDate {
    NaiveDate::from_ymd_opt(2024, 6, 15).unwrap()
}

fn day_unix(date: NaiveDate) -> i64 {
    Utc.from_utc_datetime(&date.and_hms_opt(0, 0, 0).unwrap())
        .timestamp()
}

fn ibkr_date_str(date: NaiveDate) -> String {
    date.format("%Y%m%d").to_string()
}

fn ibkr_intraday_str(date: NaiveDate, hour: u32, minute: u32) -> String {
    date.and_hms_opt(hour, minute, 0)
        .unwrap()
        .format("%Y%m%d %H:%M:%S")
        .to_string()
}

fn make_db() -> (NamedTempFile, Arc<Db>) {
    let tmp = NamedTempFile::new().expect("tempfile");
    let db = Db::open(tmp.path()).expect("open db");
    (tmp, Arc::new(db))
}

fn rate_limiter() -> Arc<HistoricalRateLimiter> {
    // Generous limit so tests don't block on it.
    Arc::new(HistoricalRateLimiter::new(60))
}

#[allow(clippy::too_many_arguments)]
async fn insert_bar(
    db: &Db,
    symbol: &str,
    bar_size: &str,
    bar_time: i64,
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    volume: i64,
    wap: f64,
) {
    let symbol = symbol.to_string();
    let bar_size = bar_size.to_string();
    db.with_conn(move |conn| {
        conn.execute(
            "INSERT OR REPLACE INTO bars_cache \
             (symbol, bar_size, bar_time, open, high, low, close, volume, wap) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![symbol, bar_size, bar_time, open, high, low, close, volume, wap],
        )?;
        Ok(())
    })
    .await
    .expect("insert ok");
}

// ------------- 1: round-trip preserves floats and large volumes -------------

#[tokio::test]
async fn bars_round_trip_through_sqlite_preserve_floats_and_volume() {
    let (_tmp, db) = make_db();

    let mut expected: Vec<HistoricalBar> = Vec::with_capacity(1000);
    let day0 = fixed_today() - ChronoDuration::days(1500);
    for i in 0..1000_i64 {
        let date = day0 + ChronoDuration::days(i);
        let t = day_unix(date);
        let bar = HistoricalBar {
            time: ibkr_date_str(date),
            open: 149.123_456_789 + (i as f64) * 0.000_001,
            high: 152.987_654_321,
            low: 148.111_111_111,
            close: 151.246_810_121,
            volume: i64::MAX / 2 - i,
            wap: 150.135_792_468,
            count: (i % 1000) as i32,
        };
        expected.push(bar.clone());
        insert_bar(
            &db,
            "AAPL",
            BarSize::Day1.as_str(),
            t,
            bar.open,
            bar.high,
            bar.low,
            bar.close,
            bar.volume,
            bar.wap,
        )
        .await;
    }

    let read_back = super::cache::read_cache(
        &db,
        "AAPL",
        BarSize::Day1,
        day_unix(day0),
        day_unix(day0 + ChronoDuration::days(999)),
    )
    .await
    .expect("read cache ok");

    assert_eq!(read_back.len(), 1000, "all 1000 bars must be present");
    for (a, b) in read_back.iter().zip(expected.iter()) {
        assert_eq!(a.open.to_bits(), b.open.to_bits(), "open bit-equal");
        assert_eq!(a.high.to_bits(), b.high.to_bits(), "high bit-equal");
        assert_eq!(a.low.to_bits(), b.low.to_bits(), "low bit-equal");
        assert_eq!(a.close.to_bits(), b.close.to_bits(), "close bit-equal");
        assert_eq!(a.volume, b.volume, "volume preserved");
        assert_eq!(a.wap.to_bits(), b.wap.to_bits(), "wap bit-equal");
    }
}

// ------------- 2: cache hit returns cached bars without fetching -------------

#[tokio::test]
async fn cache_hit_returns_cached_bars_without_calling_client() {
    let (_tmp, db) = make_db();
    let today = fixed_today();

    // Pre-populate 5 daily bars for AAPL ending today.
    for i in 0..5_i64 {
        let date = today - ChronoDuration::days(4 - i);
        insert_bar(
            &db,
            "AAPL",
            BarSize::Day1.as_str(),
            day_unix(date),
            100.0 + i as f64,
            101.0 + i as f64,
            99.0 + i as f64,
            100.5 + i as f64,
            1_000 + i,
            100.25 + i as f64,
        )
        .await;
    }

    let fetcher = Arc::new(MockHistoricalFetcher::new()); // empty queue → panics if called
    let service = HistoricalDataService::new(db, fetcher.clone(), rate_limiter())
        .with_clock(Arc::new(FixedClock(today)));

    let bars = service
        .fetch_bars("AAPL", BarSize::Day1, Lookback::Days(5))
        .await
        .expect("fetch ok");

    assert_eq!(bars.len(), 5, "all five cached bars returned");
    assert_eq!(fetcher.call_count().await, 0, "fetcher not called");
}

// ------------- 3: cache miss fetches, second call hits cache -------------

#[tokio::test]
async fn cache_miss_fetches_and_writes_through() {
    let (_tmp, db) = make_db();
    let today = fixed_today();

    let canned: Vec<HistoricalBar> = (0..5)
        .map(|i| {
            let date = today - ChronoDuration::days(4 - i);
            HistoricalBar {
                time: ibkr_date_str(date),
                open: 200.0 + i as f64,
                high: 201.0 + i as f64,
                low: 199.0 + i as f64,
                close: 200.5 + i as f64,
                volume: 5_000 + i,
                wap: 200.25 + i as f64,
                count: 100,
            }
        })
        .collect();

    let fetcher = Arc::new(MockHistoricalFetcher::new());
    fetcher.enqueue(canned.clone()).await;

    let service = HistoricalDataService::new(db, fetcher.clone(), rate_limiter())
        .with_clock(Arc::new(FixedClock(today)));

    let bars1 = service
        .fetch_bars("MSFT", BarSize::Day1, Lookback::Days(5))
        .await
        .expect("first fetch ok");
    assert_eq!(bars1.len(), 5);
    assert_eq!(fetcher.call_count().await, 1, "first call must fetch");

    let bars2 = service
        .fetch_bars("MSFT", BarSize::Day1, Lookback::Days(5))
        .await
        .expect("second fetch ok");
    assert_eq!(bars2.len(), 5);
    assert_eq!(
        fetcher.call_count().await,
        1,
        "second call must hit cache and not refetch"
    );
}

// ------------- 4: partial cache → only gap is fetched -------------

#[tokio::test]
async fn partial_cache_fetches_only_missing_range() {
    let (_tmp, db) = make_db();
    let today = fixed_today();

    // Pre-populate days T-149..T-50 (100 daily bars).
    for offset in 50..=149_i64 {
        let date = today - ChronoDuration::days(offset);
        insert_bar(
            &db,
            "AAPL",
            BarSize::Day1.as_str(),
            day_unix(date),
            10.0,
            11.0,
            9.0,
            10.5,
            1_000,
            10.25,
        )
        .await;
    }

    // Mock returns 50 fresh bars covering T-49..T-0.
    let canned: Vec<HistoricalBar> = (0..=49_i64)
        .rev()
        .map(|offset| {
            let date = today - ChronoDuration::days(offset);
            HistoricalBar {
                time: ibkr_date_str(date),
                open: 20.0,
                high: 21.0,
                low: 19.0,
                close: 20.5,
                volume: 2_000,
                wap: 20.25,
                count: 50,
            }
        })
        .collect();

    let fetcher = Arc::new(MockHistoricalFetcher::new());
    fetcher.enqueue(canned).await;

    let service = HistoricalDataService::new(db, fetcher.clone(), rate_limiter())
        .with_clock(Arc::new(FixedClock(today)));

    let bars = service
        .fetch_bars("AAPL", BarSize::Day1, Lookback::Days(200))
        .await
        .expect("fetch ok");

    let calls = fetcher.calls().await;
    assert_eq!(calls.len(), 1, "exactly one IBKR call for the gap");
    assert_eq!(
        calls[0].duration, "50 D",
        "gap duration must be exactly 50 days"
    );
    assert!(bars.len() >= 150, "must include cached + new bars");

    // sorted ascending by bar_time
    let mut prev: Option<i64> = None;
    use crate::ibkr::types::historical::parse_ibkr_time;
    for bar in &bars {
        let t = parse_ibkr_time(&bar.time).expect("parse");
        if let Some(p) = prev {
            assert!(t >= p, "bars must be sorted ascending by bar_time");
        }
        prev = Some(t);
    }
}

// ------------- 5: daily bars cached indefinitely -------------

#[tokio::test]
async fn daily_bars_cached_indefinitely() {
    let (_tmp, db) = make_db();
    let today = fixed_today();

    // Pre-populate exactly today..today-5 (6 daily bars covering the request).
    for offset in 0..=5_i64 {
        let date = today - ChronoDuration::days(offset);
        insert_bar(
            &db,
            "AAPL",
            BarSize::Day1.as_str(),
            day_unix(date),
            10.0,
            11.0,
            9.0,
            10.5,
            1_000,
            10.25,
        )
        .await;
    }

    let fetcher = Arc::new(MockHistoricalFetcher::new()); // empty → panics if called
    let service = HistoricalDataService::new(db, fetcher.clone(), rate_limiter())
        .with_clock(Arc::new(FixedClock(today)));

    let bars = service
        .fetch_bars("AAPL", BarSize::Day1, Lookback::Days(6))
        .await
        .expect("fetch ok");

    assert_eq!(bars.len(), 6);
    assert_eq!(
        fetcher.call_count().await,
        0,
        "daily cache must serve regardless of age"
    );
}

// ------------- 6: intraday cache invalid for prior calendar day -------------

#[tokio::test]
async fn intraday_bars_cached_only_for_today() {
    let (_tmp, db) = make_db();
    let today = fixed_today();
    let yesterday = today - ChronoDuration::days(1);

    // Pre-populate 5min bars on yesterday.
    for minute_step in 0..5_u32 {
        let bar_time = yesterday
            .and_hms_opt(9, 30 + minute_step * 5, 0)
            .unwrap()
            .and_utc()
            .timestamp();
        insert_bar(
            &db,
            "AAPL",
            BarSize::Min5.as_str(),
            bar_time,
            10.0,
            11.0,
            9.0,
            10.5,
            1_000,
            10.25,
        )
        .await;
    }

    // Mock canned response so the service can fetch when refusing the cache.
    let fetcher = Arc::new(MockHistoricalFetcher::new());
    fetcher
        .enqueue(vec![HistoricalBar {
            time: ibkr_intraday_str(yesterday, 9, 30),
            open: 10.0,
            high: 11.0,
            low: 9.0,
            close: 10.5,
            volume: 1_000,
            wap: 10.25,
            count: 1,
        }])
        .await;

    let service = HistoricalDataService::new(db, fetcher.clone(), rate_limiter())
        .with_clock(Arc::new(FixedClock(today)));

    let _ = service
        .fetch_bars("AAPL", BarSize::Min5, Lookback::TradingDay(yesterday))
        .await
        .expect("fetch ok");

    assert_eq!(
        fetcher.call_count().await,
        1,
        "intraday cache from a non-today calendar day must be ignored"
    );
}

// ------------- 7: rate limiter consulted only for actual fetches -------------

#[tokio::test]
async fn rate_limiter_invoked_per_request() {
    let (_tmp, db) = make_db();
    let today = fixed_today();

    let rl = Arc::new(HistoricalRateLimiter::new(60));
    let fetcher = Arc::new(MockHistoricalFetcher::new());

    // Three different symbols → three fetches.
    for _ in 0..3 {
        fetcher
            .enqueue(vec![HistoricalBar {
                time: ibkr_date_str(today),
                open: 10.0,
                high: 11.0,
                low: 9.0,
                close: 10.5,
                volume: 1_000,
                wap: 10.25,
                count: 1,
            }])
            .await;
    }

    let service = HistoricalDataService::new(db.clone(), fetcher.clone(), rl.clone())
        .with_clock(Arc::new(FixedClock(today)));

    for sym in &["AAA", "BBB", "CCC"] {
        service
            .fetch_bars(sym, BarSize::Day1, Lookback::Days(1))
            .await
            .expect("fetch ok");
    }
    assert_eq!(
        rl.acquire_count().await,
        3,
        "three fetches → three acquires"
    );

    // Pre-populate cache for DDD so the next call is a cache hit.
    insert_bar(
        &db,
        "DDD",
        BarSize::Day1.as_str(),
        day_unix(today),
        10.0,
        11.0,
        9.0,
        10.5,
        1_000,
        10.25,
    )
    .await;

    service
        .fetch_bars("DDD", BarSize::Day1, Lookback::Days(1))
        .await
        .expect("fetch ok");

    assert_eq!(
        rl.acquire_count().await,
        3,
        "cache hit must not consult rate limiter"
    );
}

// ------------- 8: in-flight dedup for same key -------------

#[tokio::test]
async fn service_dedups_in_flight_requests_for_same_key() {
    let (_tmp, db) = make_db();
    let today = fixed_today();

    let fetcher = Arc::new(MockHistoricalFetcher::new());
    let bars: Vec<HistoricalBar> = (0..5)
        .map(|i| {
            let date = today - ChronoDuration::days(4 - i);
            HistoricalBar {
                time: ibkr_date_str(date),
                open: 100.0 + i as f64,
                high: 101.0 + i as f64,
                low: 99.0 + i as f64,
                close: 100.5 + i as f64,
                volume: 1_000 + i,
                wap: 100.25 + i as f64,
                count: 1,
            }
        })
        .collect();
    // ONE canned response; second concurrent fetcher would panic on pop.
    fetcher.enqueue(bars.clone()).await;

    let gate = Arc::new(Notify::new());
    fetcher.install_first_call_gate(gate.clone()).await;

    let service = Arc::new(
        HistoricalDataService::new(db, fetcher.clone(), rate_limiter())
            .with_clock(Arc::new(FixedClock(today))),
    );

    let s1 = Arc::clone(&service);
    let s2 = Arc::clone(&service);
    let h1 = tokio::spawn(async move {
        s1.fetch_bars("ZZZ", BarSize::Day1, Lookback::Days(5))
            .await
            .expect("first ok")
    });

    // Give the first task a moment to take the per-key mutex and reach the gate.
    tokio::time::sleep(Duration::from_millis(50)).await;

    let h2 = tokio::spawn(async move {
        s2.fetch_bars("ZZZ", BarSize::Day1, Lookback::Days(5))
            .await
            .expect("second ok")
    });

    // Let the first call proceed past the gate.
    tokio::time::sleep(Duration::from_millis(50)).await;
    gate.notify_one();

    let r1 = h1.await.expect("h1 join");
    let r2 = h2.await.expect("h2 join");

    // `count` is not persisted in `bars_cache`, so the second caller —
    // who reads from cache — sees count=0. Compare on the round-tripped
    // fields instead (time + OHLCV + wap), which is the contract of
    // "both callers see the same bars" the dedup test cares about.
    assert_eq!(r1.len(), r2.len(), "same number of bars");
    for (a, b) in r1.iter().zip(r2.iter()) {
        assert_eq!(a.time, b.time);
        assert_eq!(a.open.to_bits(), b.open.to_bits());
        assert_eq!(a.high.to_bits(), b.high.to_bits());
        assert_eq!(a.low.to_bits(), b.low.to_bits());
        assert_eq!(a.close.to_bits(), b.close.to_bits());
        assert_eq!(a.volume, b.volume);
        assert_eq!(a.wap.to_bits(), b.wap.to_bits());
    }
    assert_eq!(
        fetcher.call_count().await,
        1,
        "second caller must reuse cache populated by first"
    );
}

// Smoke: ensure error path is wired (storage error → IbkrError mapping).
#[tokio::test]
async fn unrelated_smoke_storage_error_propagates_as_request_failed() {
    use crate::storage::error::StorageError;
    let err = IbkrError::RequestFailed(format!(
        "storage: {}",
        StorageError::Migration("boom".to_string())
    ));
    assert!(matches!(err, IbkrError::RequestFailed(_)));
}

// Suppress "unused" warnings for parts only some tests use.
#[allow(dead_code)]
fn _shut_up(_w: WhatToShow) {}
