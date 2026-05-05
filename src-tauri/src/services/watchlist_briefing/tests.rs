//! Composer tests — uses a `BriefingFetchers` value built from
//! closures so we can drive each underlying read in isolation.

use super::*;
use serde_json::json;

#[tokio::test]
async fn composes_one_briefing_per_symbol() {
    let fetchers = test_fetchers_ok();
    let out = compose(
        vec!["AMD".to_string(), "TSLA".to_string()],
        BriefingOpts {
            lookback_days: 15,
            bars_size: "1d".into(),
            news_max_age_secs: 3600,
            concurrency: 2,
        },
        &fetchers,
    )
    .await;

    assert_eq!(out.items.len(), 2);
    let amd = out.items.iter().find(|i| i.symbol == "AMD").unwrap();
    assert!(amd.errors.is_empty());
    assert!(amd.quote.is_some());
    assert!(amd.bars.is_some());
    assert!(amd.news.is_some());
    assert!(amd.sentiment.is_some());
    assert!(amd.setups.is_some());
    assert!(amd.fundamentals.is_some());
}

#[tokio::test]
async fn partial_failure_isolates_per_field() {
    let mut f = test_fetchers_ok();
    f.fetch_news = Box::new(|_sym| Box::pin(async { Err("upstream_failed".into()) }));
    let out = compose(vec!["AMD".into()], BriefingOpts::default(), &f).await;
    assert_eq!(out.items.len(), 1);
    let it = &out.items[0];
    assert!(it.news.is_none(), "news should be missing");
    assert!(it.quote.is_some(), "quote should still be present");
    assert!(
        it.errors
            .iter()
            .any(|e| e.contains("news") && e.contains("upstream_failed")),
        "errors: {:?}",
        it.errors,
    );
}

#[tokio::test]
async fn empty_symbol_list_returns_empty_items() {
    let out = compose(vec![], BriefingOpts::default(), &test_fetchers_ok()).await;
    assert!(out.items.is_empty());
}

#[tokio::test]
async fn items_sorted_alphabetically() {
    let out = compose(
        vec!["TSLA".into(), "AMD".into(), "RDDT".into()],
        BriefingOpts::default(),
        &test_fetchers_ok(),
    )
    .await;
    let order: Vec<_> = out.items.iter().map(|i| i.symbol.as_str()).collect();
    assert_eq!(order, vec!["AMD", "RDDT", "TSLA"]);
}

#[tokio::test]
async fn bars_fetcher_receives_size_and_lookback() {
    use std::sync::Arc;
    use std::sync::Mutex;
    let captured: Arc<Mutex<Vec<(String, u32)>>> = Arc::new(Mutex::new(Vec::new()));
    let cap = Arc::clone(&captured);
    let mut f = test_fetchers_ok();
    f.fetch_bars = Box::new(move |_sym, size, lookback| {
        let cap = Arc::clone(&cap);
        let size = size.to_string();
        Box::pin(async move {
            cap.lock().unwrap().push((size, lookback));
            Ok(json!({"items": []}))
        })
    });
    let _ = compose(
        vec!["AMD".into()],
        BriefingOpts {
            lookback_days: 30,
            bars_size: "1h".into(),
            news_max_age_secs: 3600,
            concurrency: 1,
        },
        &f,
    )
    .await;
    let cap = captured.lock().unwrap();
    assert_eq!(cap.as_slice(), &[("1h".to_string(), 30u32)]);
}

fn test_fetchers_ok() -> BriefingFetchers {
    BriefingFetchers {
        fetch_quote: Box::new(|sym| {
            let s = sym.to_string();
            Box::pin(async move { Ok(json!({"symbol": s, "lastPrice": 100.0})) })
        }),
        fetch_bars: Box::new(|_sym, _size, _lookback| {
            Box::pin(async { Ok(json!({"items": [], "count": 0})) })
        }),
        fetch_news: Box::new(|_sym| Box::pin(async { Ok(json!({"items": []})) })),
        fetch_sentiment: Box::new(|_sym| Box::pin(async { Ok(json!({"items": []})) })),
        fetch_setups: Box::new(|_sym| Box::pin(async { Ok(json!({"items": [], "count": 0})) })),
        fetch_fundamentals: Box::new(|_sym| Box::pin(async { Ok(json!(null)) })),
    }
}
