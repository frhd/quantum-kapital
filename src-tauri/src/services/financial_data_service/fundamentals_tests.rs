//! Tests for the fundamentals path of `FinancialDataService` introduced
//! in Phase 1 of the AV → IBKR migration:
//!
//! - In-flight coalescing: concurrent calls for the same symbol fan out
//!   to AV exactly once.
//! - Stale-cache fallback: rate-limit (Information) responses serve the
//!   most recently cached payload instead of erroring out.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{json, Value};
use tempfile::TempDir;

use super::{AvHttp, AvHttpError, FinancialDataService};
use crate::services::cache_service::CacheService;

// ---------- fixtures ----------

fn overview_json() -> Value {
    json!({
        "Symbol": "AAPL",
        "Name": "Apple Inc.",
        "Exchange": "NASDAQ",
        "MarketCapitalization": "3000000000000",
        "PERatio": "30.0",
        "SharesOutstanding": "15000000000",
        "52WeekHigh": "200.00",
        "DividendYield": "0.005"
    })
}

fn income_json() -> Value {
    json!({
        "symbol": "AAPL",
        "annualReports": [
            {
                "fiscalDateEnding": "2024-09-30",
                "totalRevenue": "390000000000",
                "netIncome": "100000000000"
            },
            {
                "fiscalDateEnding": "2023-09-30",
                "totalRevenue": "380000000000",
                "netIncome": "90000000000"
            }
        ]
    })
}

fn earnings_json() -> Value {
    json!({
        "symbol": "AAPL",
        "annualEarnings": [
            {"fiscalDateEnding": "2024-09-30", "reportedEPS": "6.5"},
            {"fiscalDateEnding": "2023-09-30", "reportedEPS": "6.0"}
        ],
        "quarterlyEarnings": []
    })
}

fn rate_limit_payload() -> Value {
    json!({
        "Information": "Thank you for using Alpha Vantage! Our standard API rate limit is 25 requests per day. Please subscribe to any of the premium plans..."
    })
}

// ---------- AvHttp test doubles ----------

/// Routes responses by inspecting the AV `function=` query param so a
/// single fake can serve all three endpoint URLs.
struct RoutedAvHttp {
    counter: Arc<AtomicUsize>,
    delay: Duration,
    overview: Value,
    income: Value,
    earnings: Value,
}

impl RoutedAvHttp {
    fn new(delay: Duration) -> Self {
        Self {
            counter: Arc::new(AtomicUsize::new(0)),
            delay,
            overview: overview_json(),
            income: income_json(),
            earnings: earnings_json(),
        }
    }
}

#[async_trait]
impl AvHttp for RoutedAvHttp {
    async fn fetch(&self, url: &str) -> Result<Value, AvHttpError> {
        self.counter.fetch_add(1, Ordering::SeqCst);
        if !self.delay.is_zero() {
            tokio::time::sleep(self.delay).await;
        }
        Ok(if url.contains("function=OVERVIEW") {
            self.overview.clone()
        } else if url.contains("function=INCOME_STATEMENT") {
            self.income.clone()
        } else if url.contains("function=EARNINGS") {
            self.earnings.clone()
        } else {
            return Err(AvHttpError::Status(format!("unexpected url: {url}")));
        })
    }
}

/// Always returns the AV rate-limit `Information` payload regardless
/// of which function is requested. Used by the stale-cache fallback test.
struct AlwaysRateLimited {
    counter: Arc<AtomicUsize>,
}

#[async_trait]
impl AvHttp for AlwaysRateLimited {
    async fn fetch(&self, _url: &str) -> Result<Value, AvHttpError> {
        self.counter.fetch_add(1, Ordering::SeqCst);
        Ok(rate_limit_payload())
    }
}

// ---------- coalescing ----------

#[tokio::test]
async fn ten_concurrent_fetches_fan_out_to_av_exactly_once() {
    // No cache → every miss-then-call would go to AV; coalescing is
    // the only thing keeping the request count to 3.
    let temp = TempDir::new().unwrap();
    let cache = CacheService::new(temp.path()).unwrap();
    let fake = Arc::new(RoutedAvHttp::new(Duration::from_millis(80)));
    let counter = Arc::clone(&fake.counter);
    let svc = Arc::new(
        FinancialDataService::new("KEY".into())
            .with_http(fake.clone() as Arc<dyn AvHttp>)
            .with_cache(cache),
    );

    let mut handles = Vec::new();
    for _ in 0..10 {
        let s = Arc::clone(&svc);
        handles.push(tokio::spawn(async move {
            s.fetch_fundamental_data("AAPL").await
        }));
    }

    let mut ok = 0;
    for h in handles {
        let result = h.await.expect("task did not panic");
        assert!(result.is_ok(), "fetch_fundamental_data err: {result:?}");
        ok += 1;
    }
    assert_eq!(ok, 10);

    let calls = counter.load(Ordering::SeqCst);
    assert_eq!(
        calls, 3,
        "10 concurrent callers must coalesce to exactly 3 AV requests (overview/income/earnings); got {calls}"
    );
}

#[tokio::test]
async fn failed_leader_does_not_poison_the_slot() {
    // First fetch errors (transport); flip the toggle and the next
    // fetch must be free to start a fresh attempt and succeed. Catches
    // a regression where the in-flight slot would stay populated after
    // a failed leader, leaving subsequent callers wedged on a closed
    // broadcast.
    struct ToggleableFake {
        fail: AtomicBool,
        overview: Value,
        income: Value,
        earnings: Value,
    }

    #[async_trait]
    impl AvHttp for ToggleableFake {
        async fn fetch(&self, url: &str) -> Result<Value, AvHttpError> {
            if self.fail.load(Ordering::SeqCst) {
                return Err(AvHttpError::Transport("simulated".into()));
            }
            Ok(if url.contains("function=OVERVIEW") {
                self.overview.clone()
            } else if url.contains("function=INCOME_STATEMENT") {
                self.income.clone()
            } else {
                self.earnings.clone()
            })
        }
    }

    let temp = TempDir::new().unwrap();
    let cache = CacheService::new(temp.path()).unwrap();
    let fake = Arc::new(ToggleableFake {
        fail: AtomicBool::new(true),
        overview: overview_json(),
        income: income_json(),
        earnings: earnings_json(),
    });
    let svc = FinancialDataService::new("KEY".into())
        .with_http(fake.clone() as Arc<dyn AvHttp>)
        .with_cache(cache);

    let first = svc.fetch_fundamental_data("AAPL").await;
    assert!(first.is_err(), "first call should error: {first:?}");

    fake.fail.store(false, Ordering::SeqCst);

    let second = svc.fetch_fundamental_data("AAPL").await;
    assert!(
        second.is_ok(),
        "after failed leader, retry must succeed: {second:?}"
    );
}

// ---------- stale-cache fallback ----------

#[tokio::test]
async fn stale_cache_fallback_serves_expired_payload_on_information_rate_limit() {
    // 1s TTL so we can prime the cache, sleep past it, and force the
    // top-of-fetch_av_function cache check to miss.
    let temp = TempDir::new().unwrap();
    let cache = CacheService::with_ttl(temp.path(), Duration::from_secs(1)).unwrap();

    // Prime all three cache keys with parseable AV payloads (we write
    // them under their AV cache_suffix names so the production code
    // path picks them up).
    cache.write("AAPL_overview", &overview_json()).unwrap();
    cache
        .write("AAPL_income_statement", &income_json())
        .unwrap();
    cache.write("AAPL_earnings", &earnings_json()).unwrap();

    // Wait past the TTL.
    tokio::time::sleep(Duration::from_secs(2)).await;

    let counter = Arc::new(AtomicUsize::new(0));
    let fake = Arc::new(AlwaysRateLimited {
        counter: Arc::clone(&counter),
    });
    let svc = FinancialDataService::new("KEY".into())
        .with_http(fake.clone() as Arc<dyn AvHttp>)
        .with_cache(cache);

    let result = svc
        .fetch_fundamental_data("AAPL")
        .await
        .expect("rate-limit must fall back to stale cache, not error");

    assert_eq!(result.symbol, "AAPL");
    assert!(
        !result.historical.is_empty(),
        "stale cache must yield historical rows"
    );
    // All three endpoints attempted (and all returned Information).
    let calls = counter.load(Ordering::SeqCst);
    assert!(
        calls >= 1,
        "AV must have been called past the cache miss; got {calls}"
    );
}
