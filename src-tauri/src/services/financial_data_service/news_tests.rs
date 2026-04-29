use std::sync::Arc;
use std::sync::Mutex;

use async_trait::async_trait;
use serde_json::{json, Value};
use tempfile::NamedTempFile;

use crate::ibkr::types::news::NewsItem;
use crate::storage::Db;

use super::news::{
    fetch_news_sentiment_with_deps, parse_news_response, NewsClock, NewsHttp, NewsHttpError,
};

// ---------------- helpers ----------------

const FIXTURE: &str = include_str!("../../../tests/fixtures/av_news_sentiment.json");

fn fixture_json() -> Value {
    serde_json::from_str(FIXTURE).expect("fixture parses")
}

fn make_db() -> (NamedTempFile, Arc<Db>) {
    let tmp = NamedTempFile::new().expect("tempfile");
    let db = Db::open(tmp.path()).expect("open db");
    (tmp, Arc::new(db))
}

#[derive(Default)]
struct MockHttp {
    canned: Mutex<std::collections::VecDeque<Result<Value, NewsHttpError>>>,
    call_count: Mutex<usize>,
}

impl MockHttp {
    fn new() -> Self {
        Self::default()
    }
    fn enqueue_ok(&self, value: Value) {
        self.canned.lock().unwrap().push_back(Ok(value));
    }
    #[allow(dead_code)]
    fn enqueue_err(&self, err: NewsHttpError) {
        self.canned.lock().unwrap().push_back(Err(err));
    }
    fn calls(&self) -> usize {
        *self.call_count.lock().unwrap()
    }
}

#[async_trait]
impl NewsHttp for MockHttp {
    async fn fetch(&self, _url: &str) -> Result<Value, NewsHttpError> {
        *self.call_count.lock().unwrap() += 1;
        self.canned
            .lock()
            .unwrap()
            .pop_front()
            .expect("MockHttp queue exhausted")
    }
}

struct FixedClock(std::sync::atomic::AtomicI64);

impl FixedClock {
    fn new(now: i64) -> Self {
        Self(std::sync::atomic::AtomicI64::new(now))
    }
    fn advance(&self, seconds: i64) {
        self.0
            .fetch_add(seconds, std::sync::atomic::Ordering::Relaxed);
    }
}

impl NewsClock for FixedClock {
    fn now_unix(&self) -> i64 {
        self.0.load(std::sync::atomic::Ordering::Relaxed)
    }
}

async fn write_cache_row(db: &Db, symbol: &str, fetched_at: i64, items: &[NewsItem]) {
    let symbol = symbol.to_string();
    let payload = serde_json::to_string(items).expect("serialize");
    db.with_conn(move |conn| {
        conn.execute(
            "INSERT OR REPLACE INTO news_cache (symbol, fetched_at, payload) VALUES (?1, ?2, ?3)",
            rusqlite::params![symbol, fetched_at, payload],
        )?;
        Ok(())
    })
    .await
    .expect("write cache");
}

async fn read_cache_row(db: &Db, symbol: &str) -> Option<(i64, Vec<NewsItem>)> {
    let symbol = symbol.to_string();
    db.with_conn(move |conn| {
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
    .await
    .expect("read cache")
    .map(|(fa, payload)| {
        let items: Vec<NewsItem> = serde_json::from_str(&payload).expect("deserialize");
        (fa, items)
    })
}

const TTL_60_MIN_SECS: i64 = 60 * 60;

// ---------------- 1: parses fixture ----------------

#[test]
fn parses_news_sentiment_response() {
    let json = fixture_json();
    let items = parse_news_response(&json, "NVDA");

    // Two items mention NVDA in ticker_sentiment.
    assert_eq!(items.len(), 2, "two NVDA items expected");

    let first = &items[0];
    assert_eq!(first.title, "NVIDIA AI Chip Demand Soars Past Expectations");
    assert_eq!(first.source, "Reuters");
    assert_eq!(first.url, "https://example.com/articles/nvda-ai-demand");
    assert_eq!(first.overall_sentiment_score, Some(0.567892));
    assert_eq!(first.overall_sentiment_label.as_deref(), Some("Bullish"));
    assert_eq!(first.ticker_sentiment.len(), 2);
    let nvda_ts = first
        .ticker_sentiment
        .iter()
        .find(|t| t.ticker == "NVDA")
        .expect("nvda ticker_sentiment");
    assert!((nvda_ts.relevance_score - 0.91).abs() < 1e-9);
    assert!((nvda_ts.ticker_sentiment_score - 0.66).abs() < 1e-9);
    assert_eq!(nvda_ts.ticker_sentiment_label, "Bullish");

    // Time was 2025-01-15 14:30:00 UTC
    assert_eq!(
        first.time_published.format("%Y-%m-%dT%H:%M:%S").to_string(),
        "2025-01-15T14:30:00"
    );
}

// ---------------- 6: filtered to requested symbol ----------------

#[test]
fn ticker_sentiment_is_filtered_to_requested_symbol() {
    let json = fixture_json();

    let nvda_items = parse_news_response(&json, "NVDA");
    assert!(nvda_items
        .iter()
        .all(|i| i.ticker_sentiment.iter().any(|t| t.ticker == "NVDA")));
    assert!(
        !nvda_items
            .iter()
            .any(|i| i.title.contains("AMD Earnings Miss")),
        "AMD-only article must be filtered out for NVDA"
    );

    let amd_items = parse_news_response(&json, "AMD");
    assert_eq!(amd_items.len(), 2, "two AMD-tagged items");
    assert!(amd_items
        .iter()
        .all(|i| i.ticker_sentiment.iter().any(|t| t.ticker == "AMD")));
}

// ---------------- 7: missing optional fields ----------------

#[test]
fn news_item_handles_missing_optional_fields() {
    let json = fixture_json();
    let nvda_items = parse_news_response(&json, "NVDA");

    // The third feed entry (a tech sector article) has null overall_sentiment_*.
    let with_null = nvda_items
        .iter()
        .find(|i| i.title.contains("Tech Sector"))
        .expect("tech sector item should be present in NVDA filter");

    assert!(with_null.overall_sentiment_score.is_none());
    assert!(with_null.overall_sentiment_label.is_none());
}

// ---------------- 4: cache hit within TTL skips HTTP ----------------

#[tokio::test]
async fn cache_hit_within_ttl_skips_http() {
    let (_tmp, db) = make_db();
    let now: i64 = 1_700_000_000;

    let cached_items = vec![NewsItem {
        time_published: chrono::Utc.timestamp_opt(now, 0).unwrap(),
        title: "cached".to_string(),
        summary: "cached summary".to_string(),
        source: "Cache".to_string(),
        url: "https://cache.example/1".to_string(),
        overall_sentiment_score: Some(0.5),
        overall_sentiment_label: Some("Bullish".to_string()),
        ticker_sentiment: vec![],
    }];
    write_cache_row(&db, "NVDA", now - 30, &cached_items).await;

    let http = MockHttp::new(); // empty queue → would panic if called
    let clock = FixedClock::new(now);

    let result = fetch_news_sentiment_with_deps(
        &http,
        &clock,
        &db,
        "TESTKEY",
        "https://www.alphavantage.co/query",
        "NVDA",
        24,
        TTL_60_MIN_SECS,
    )
    .await
    .expect("ok");

    assert_eq!(result.len(), 1, "cached payload returned");
    assert_eq!(result[0].title, "cached");
    assert_eq!(http.calls(), 0, "HTTP must not be called within TTL");
}

// ---------------- 5: cache miss after TTL refetches ----------------

#[tokio::test]
async fn cache_miss_after_ttl_refetches() {
    let (_tmp, db) = make_db();
    let initial_now: i64 = 1_700_000_000;

    let stale_items = vec![NewsItem {
        time_published: chrono::Utc.timestamp_opt(initial_now, 0).unwrap(),
        title: "stale".to_string(),
        summary: "stale summary".to_string(),
        source: "Cache".to_string(),
        url: "https://cache.example/old".to_string(),
        overall_sentiment_score: None,
        overall_sentiment_label: None,
        ticker_sentiment: vec![],
    }];
    write_cache_row(&db, "NVDA", initial_now, &stale_items).await;

    let http = MockHttp::new();
    http.enqueue_ok(fixture_json());

    let clock = FixedClock::new(initial_now);
    // Advance well past 60-min TTL.
    clock.advance(TTL_60_MIN_SECS + 60);

    let result = fetch_news_sentiment_with_deps(
        &http,
        &clock,
        &db,
        "TESTKEY",
        "https://www.alphavantage.co/query",
        "NVDA",
        24,
        TTL_60_MIN_SECS,
    )
    .await
    .expect("ok");

    assert_eq!(http.calls(), 1, "HTTP must refetch after TTL");
    assert_eq!(
        result.len(),
        2,
        "fixture has two NVDA items, both returned post-refetch"
    );

    // Cache row should be updated to the new fetch time.
    let row = read_cache_row(&db, "NVDA").await.expect("cache row exists");
    assert_eq!(row.0, clock.now_unix(), "fetched_at refreshed");
}

// ---------------- 2: rate-limited fallback to cache ----------------

#[tokio::test]
async fn falls_back_to_cached_when_rate_limited() {
    let (_tmp, db) = make_db();
    let now: i64 = 1_700_000_000;

    let cached_items = vec![NewsItem {
        time_published: chrono::Utc.timestamp_opt(now - 100_000, 0).unwrap(),
        title: "old cached".to_string(),
        summary: "summary".to_string(),
        source: "Cache".to_string(),
        url: "https://cache.example/2".to_string(),
        overall_sentiment_score: None,
        overall_sentiment_label: None,
        ticker_sentiment: vec![],
    }];
    // Stale: fetched_at far in the past so cache is past TTL.
    write_cache_row(&db, "NVDA", now - 100_000, &cached_items).await;

    let http = MockHttp::new();
    http.enqueue_ok(json!({
        "Note": "Thank you for using Alpha Vantage! Our standard API rate limit is 25 requests per day."
    }));

    let clock = FixedClock::new(now);

    let result = fetch_news_sentiment_with_deps(
        &http,
        &clock,
        &db,
        "TESTKEY",
        "https://www.alphavantage.co/query",
        "NVDA",
        24,
        TTL_60_MIN_SECS,
    )
    .await
    .expect("must not propagate rate-limit as an error");

    assert_eq!(http.calls(), 1, "HTTP attempted once");
    assert_eq!(result.len(), 1, "stale cache returned as fallback");
    assert_eq!(result[0].title, "old cached");
}

// ---------------- 3: rate-limited with no cache returns empty ----------------

#[tokio::test]
async fn falls_back_to_empty_when_no_cache_and_rate_limited() {
    let (_tmp, db) = make_db();
    let now: i64 = 1_700_000_000;

    let http = MockHttp::new();
    http.enqueue_ok(json!({
        "Note": "rate-limited"
    }));

    let clock = FixedClock::new(now);

    let result = fetch_news_sentiment_with_deps(
        &http,
        &clock,
        &db,
        "TESTKEY",
        "https://www.alphavantage.co/query",
        "NVDA",
        24,
        TTL_60_MIN_SECS,
    )
    .await
    .expect("ok (best-effort)");

    assert_eq!(http.calls(), 1, "HTTP attempted once");
    assert!(result.is_empty(), "no cache → empty");
}

#[allow(unused_imports)]
use chrono::TimeZone;
