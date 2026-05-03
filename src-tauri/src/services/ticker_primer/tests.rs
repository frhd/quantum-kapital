//! Unit tests for `TickerPrimerService`. Covers the four shapes called out
//! in the Phase 1 plan: a fresh prime fans out to all three sub-services
//! and stamps `last_primed_at`; a re-prime within 24h short-circuits with
//! no provider calls; archive clears the watermark so re-prime fires; a
//! "no fundamentals" upstream still produces a non-error outcome and
//! still warms news.
//!
//!
//! Tests use the existing `FakeFundamentalsProvider` / `FakeNewsProvider`
//! seams plus a `TempDir`-backed `CacheService` so no real IBKR client is
//! involved (Hard Invariant #5: mock-friendly trait seams unchanged).

use std::sync::Arc;

use tempfile::{NamedTempFile, TempDir};

use crate::events::{AppEvent, EventEmitter};
use crate::ibkr::types::news::NewsItem;
use crate::ibkr::types::tracker::{TickerPrimingStepStatus, TrackerSource};
use crate::ibkr::types::{
    AnalystEstimate, AnalystEstimates, CurrentMetrics, FundamentalData, HistoricalFinancial,
    ProjectionResults,
};
use crate::services::cache_service::CacheService;
use crate::services::fundamentals_provider::test_support::FakeFundamentalsProvider;
use crate::services::fundamentals_provider::FundamentalsProvider;
use crate::services::news_provider::test_support::FakeNewsProvider;
use crate::services::news_provider::NewsProvider;
use crate::services::tracker_service::TrackerService;
use crate::storage::Db;

use super::{projection_cache_key, TickerPrimerService};

/// One-stop fixture: temp DB, temp cache dir, programmable fundamentals
/// and news fakes, and an `EventEmitter` with capture pre-enabled so
/// the outcome events can be asserted on without standing up Tauri.
struct Fixture {
    _db_tmp: NamedTempFile,
    _cache_tmp: TempDir,
    primer: Arc<TickerPrimerService>,
    tracker: Arc<TrackerService>,
    fundamentals: Arc<FakeFundamentalsProvider>,
    news: Arc<FakeNewsProvider>,
    cache: Arc<CacheService>,
    emitter: Arc<EventEmitter>,
}

impl Fixture {
    fn new() -> Self {
        let db_tmp = NamedTempFile::new().expect("tempfile");
        let db = Arc::new(Db::open(db_tmp.path()).expect("open db"));
        let tracker = Arc::new(TrackerService::new(Arc::clone(&db)));

        let cache_tmp = TempDir::new().expect("temp cache dir");
        let cache = Arc::new(CacheService::new(cache_tmp.path()).expect("cache"));

        let fundamentals = Arc::new(FakeFundamentalsProvider::new());
        let news = Arc::new(FakeNewsProvider::new());
        let emitter = Arc::new(EventEmitter::for_capture());

        let primer = Arc::new(TickerPrimerService::new(
            Arc::clone(&tracker),
            Arc::clone(&fundamentals) as Arc<dyn FundamentalsProvider>,
            Arc::clone(&news) as Arc<dyn NewsProvider>,
            Arc::clone(&cache),
            Arc::clone(&emitter),
        ));

        Self {
            _db_tmp: db_tmp,
            _cache_tmp: cache_tmp,
            primer,
            tracker,
            fundamentals,
            news,
            cache,
            emitter,
        }
    }
}

fn sample_fundamentals(symbol: &str) -> FundamentalData {
    FundamentalData {
        symbol: symbol.to_string(),
        historical: vec![
            HistoricalFinancial {
                year: 2023,
                revenue: 60.0,
                net_income: 30.0,
                eps: 1.2,
            },
            HistoricalFinancial {
                year: 2024,
                revenue: 130.0,
                net_income: 70.0,
                eps: 2.9,
            },
        ],
        analyst_estimates: Some(AnalystEstimates {
            revenue: vec![AnalystEstimate {
                year: 2025,
                estimate: 170.0,
            }],
            eps: vec![AnalystEstimate {
                year: 2025,
                estimate: 3.5,
            }],
        }),
        current_metrics: CurrentMetrics {
            price: Some(120.0),
            pe_ratio: 60.0,
            shares_outstanding: 24000.0,
            name: Some(format!("{symbol} Corp")),
            exchange: Some("NASDAQ".into()),
            market_cap: Some("3T".into()),
            dividend_yield: None,
        },
    }
}

fn sample_news_item(symbol: &str) -> NewsItem {
    NewsItem {
        time_published: chrono::Utc::now(),
        title: format!("{symbol} headline"),
        summary: format!("{symbol} did a thing"),
        source: "BRFG".into(),
        url: format!("https://example.com/{symbol}"),
        overall_sentiment_score: None,
        overall_sentiment_label: None,
        ticker_sentiment: vec![],
    }
}

#[tokio::test]
async fn prime_fresh_symbol_runs_all_steps_and_stamps_watermark() {
    let fx = Fixture::new();
    fx.tracker
        .add("AAPL", TrackerSource::Manual, None, vec![], None)
        .await
        .unwrap();
    fx.fundamentals.insert("AAPL", sample_fundamentals("AAPL"));
    fx.news.insert("AAPL", vec![sample_news_item("AAPL")]);

    let outcome = fx.primer.prime("aapl").await;
    assert_eq!(outcome.fundamentals, TickerPrimingStepStatus::Ok);
    assert_eq!(outcome.projection, TickerPrimingStepStatus::Ok);
    assert_eq!(outcome.news, TickerPrimingStepStatus::Ok);

    // Watermark stamped.
    let row = fx.tracker.get("AAPL").await.unwrap().expect("present");
    assert!(
        row.last_primed_at.is_some(),
        "fresh prime must stamp last_primed_at"
    );

    // Projection cache populated under the namespaced key.
    let cached: ProjectionResults = fx
        .cache
        .read(&projection_cache_key("AAPL"))
        .expect("projection cached");
    assert!(!cached.projections.is_empty());

    // Outcome event emitted.
    let events = fx.emitter.captured().await;
    let found = events
        .iter()
        .any(|e| matches!(e, AppEvent::TickerPrimingDone { symbol, .. } if symbol == "AAPL"));
    assert!(found, "TickerPrimingDone must be emitted for fresh prime");
}

#[tokio::test]
async fn prime_within_24h_is_a_noop() {
    let fx = Fixture::new();
    fx.tracker
        .add("AAPL", TrackerSource::Manual, None, vec![], None)
        .await
        .unwrap();
    fx.fundamentals.insert("AAPL", sample_fundamentals("AAPL"));
    fx.news.insert("AAPL", vec![sample_news_item("AAPL")]);

    let _ = fx.primer.prime("AAPL").await;

    // Swap providers for spies that fail loudly if called: any
    // `fail_with` payload makes the adapter return `Err(Other(_))`,
    // which would surface as a non-Skipped status in the outcome.
    fx.fundamentals.fail_with("must not be called");
    fx.news.fail_with("must not be called");

    let second = fx.primer.prime("AAPL").await;
    assert_eq!(
        second.fundamentals,
        TickerPrimingStepStatus::Skipped,
        "re-prime within 24h must short-circuit"
    );
    assert_eq!(second.projection, TickerPrimingStepStatus::Skipped);
    assert_eq!(second.news, TickerPrimingStepStatus::Skipped);
}

#[tokio::test]
async fn archive_clears_watermark_so_reprime_runs() {
    let fx = Fixture::new();
    fx.tracker
        .add("AAPL", TrackerSource::Manual, None, vec![], None)
        .await
        .unwrap();
    fx.fundamentals.insert("AAPL", sample_fundamentals("AAPL"));
    fx.news.insert("AAPL", vec![sample_news_item("AAPL")]);

    let _ = fx.primer.prime("AAPL").await;
    let row = fx.tracker.get("AAPL").await.unwrap().expect("present");
    assert!(row.last_primed_at.is_some());

    // Archive must clear the column.
    fx.tracker.archive_ticker("AAPL").await.unwrap();
    fx.tracker.unarchive_ticker("AAPL").await.unwrap();
    let restored = fx.tracker.get("AAPL").await.unwrap().expect("present");
    assert!(
        restored.last_primed_at.is_none(),
        "archive_ticker must clear last_primed_at"
    );

    // A re-prime must therefore actually run all three steps again.
    let outcome = fx.primer.prime("AAPL").await;
    assert_eq!(outcome.fundamentals, TickerPrimingStepStatus::Ok);
    assert_eq!(outcome.projection, TickerPrimingStepStatus::Ok);
    assert_eq!(outcome.news, TickerPrimingStepStatus::Ok);
}

#[tokio::test]
async fn no_fundamentals_data_yields_projection_no_data_and_still_runs_news() {
    let fx = Fixture::new();
    fx.tracker
        .add("ZZZZ", TrackerSource::Manual, None, vec![], None)
        .await
        .unwrap();
    // No fundamentals insert → FakeFundamentalsProvider returns NotFound.
    fx.news.insert("ZZZZ", vec![sample_news_item("ZZZZ")]);

    let outcome = fx.primer.prime("ZZZZ").await;
    assert_eq!(outcome.fundamentals, TickerPrimingStepStatus::NoData);
    assert_eq!(
        outcome.projection,
        TickerPrimingStepStatus::NoData,
        "projection must report NoData when fundamentals returned NotFound"
    );
    assert_eq!(outcome.news, TickerPrimingStepStatus::Ok);

    // Watermark is still stamped — the primer counts the fundamentals
    // call as "completed (no data)" so a re-add doesn't loop.
    let row = fx.tracker.get("ZZZZ").await.unwrap().expect("present");
    assert!(
        row.last_primed_at.is_some(),
        "no-data fundamentals must still stamp last_primed_at"
    );
}

#[tokio::test]
async fn fundamentals_hard_failure_leaves_watermark_null() {
    let fx = Fixture::new();
    fx.tracker
        .add("AAPL", TrackerSource::Manual, None, vec![], None)
        .await
        .unwrap();
    fx.fundamentals.fail_with("transient upstream blew up");
    fx.news.insert("AAPL", vec![sample_news_item("AAPL")]);

    let outcome = fx.primer.prime("AAPL").await;
    assert!(matches!(
        outcome.fundamentals,
        TickerPrimingStepStatus::Err(_)
    ));
    assert_eq!(outcome.news, TickerPrimingStepStatus::Ok);

    let row = fx.tracker.get("AAPL").await.unwrap().expect("present");
    assert!(
        row.last_primed_at.is_none(),
        "hard fundamentals failure must NOT stamp last_primed_at — \
         the next add deserves a real attempt"
    );
}

#[tokio::test]
async fn empty_news_response_records_no_data() {
    let fx = Fixture::new();
    fx.tracker
        .add("AAPL", TrackerSource::Manual, None, vec![], None)
        .await
        .unwrap();
    fx.fundamentals.insert("AAPL", sample_fundamentals("AAPL"));
    // No news insert → FakeNewsProvider returns Ok(empty).

    let outcome = fx.primer.prime("AAPL").await;
    assert_eq!(outcome.fundamentals, TickerPrimingStepStatus::Ok);
    assert_eq!(outcome.projection, TickerPrimingStepStatus::Ok);
    assert_eq!(outcome.news, TickerPrimingStepStatus::NoData);
}
