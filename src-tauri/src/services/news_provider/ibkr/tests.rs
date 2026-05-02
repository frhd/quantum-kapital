//! Fixture-driven integration tests for [`super::IbkrNewsProvider`].
//! Phase 6 captured the AAPL fixtures we replay here; the trait-level
//! tests cover the seam separately in [`super::client::compile_checks`]
//! and the parser tests in [`super::parsers::tests`].

use std::sync::Arc;

use crate::middleware::IbkrNewsRateLimiter;
use crate::services::news_provider::NewsError;
use crate::services::news_provider::NewsProvider;

use super::client::IbkrNewsClient;
use super::test_support::{phase6_aapl_headlines, phase6_news_providers, FixtureIbkrNewsClient};
use super::IbkrNewsProvider;

fn limiter() -> Arc<IbkrNewsRateLimiter> {
    Arc::new(IbkrNewsRateLimiter::new(30))
}

#[tokio::test]
async fn fetch_returns_phase6_aapl_fixture_rows() {
    let fake = Arc::new(FixtureIbkrNewsClient::with_phase6_aapl());
    let provider = IbkrNewsProvider::new(fake, limiter());

    let items = provider.fetch("AAPL", 24).await.expect("fetch ok");
    assert!(
        !items.is_empty(),
        "Phase 6 fixture has 50 AAPL headlines — expected non-empty parse output"
    );

    let raw = phase6_aapl_headlines();
    let directory = phase6_news_providers();
    assert_eq!(items.len(), raw.len());

    // First headline in the fixture: provider_code DJ-N, headline
    // begins with `{A:800015:L:en}Review & Preview: ...`. After
    // parsing, source must be the provider name and title must have
    // the metadata block stripped.
    let first = &items[0];
    let dj_n_name = directory
        .iter()
        .find(|p| p.code == "DJ-N")
        .map(|p| p.name.clone())
        .expect("DJ-N in Phase 6 directory");
    assert_eq!(first.source, dj_n_name);
    assert!(
        !first.title.starts_with("{A:"),
        "title must not retain {{A:...}} metadata block, got {}",
        first.title
    );
    assert!(!first.title.is_empty());

    // Sentiment loss audit: per-article fields stay None / empty.
    for item in &items {
        assert!(item.overall_sentiment_score.is_none());
        assert!(item.overall_sentiment_label.is_none());
        assert!(item.ticker_sentiment.is_empty());
    }
}

#[tokio::test]
async fn fetch_uppercases_symbol_before_dispatch() {
    let fake = Arc::new(FixtureIbkrNewsClient::with_phase6_aapl());
    let provider = IbkrNewsProvider::new(fake, limiter());

    let upper = provider.fetch("AAPL", 24).await.expect("upper ok");
    let lower = provider.fetch("aapl", 24).await.expect("lower ok");
    assert_eq!(upper.len(), lower.len());
    assert_eq!(
        upper.first().map(|i| &i.title),
        lower.first().map(|i| &i.title)
    );
}

#[tokio::test]
async fn unknown_symbol_returns_empty_not_error() {
    let fake = Arc::new(FixtureIbkrNewsClient::with_phase6_aapl());
    let provider = IbkrNewsProvider::new(fake, limiter());

    let items = provider.fetch("ZZZZ", 24).await.expect("Ok(empty)");
    assert!(
        items.is_empty(),
        "no-news-for-symbol must surface as Ok(Vec::new()), not an error"
    );
}

#[tokio::test]
async fn no_subscribed_providers_surfaces_as_no_subscription() {
    let fake = Arc::new(FixtureIbkrNewsClient::new());
    fake.set_providers(Vec::new());
    let provider = IbkrNewsProvider::new(fake, limiter());

    let err = provider
        .fetch("AAPL", 24)
        .await
        .expect_err("zero subscriptions must error");
    assert!(matches!(err, NewsError::NoSubscription { .. }));
}

#[tokio::test]
async fn historical_news_subscription_denial_propagates() {
    let fake = Arc::new(FixtureIbkrNewsClient::with_phase6_aapl());
    fake.fail_historical_news(NewsError::NoSubscription {
        provider_code: "DJ-N".to_string(),
    });
    let provider = IbkrNewsProvider::new(fake, limiter());

    let err = provider
        .fetch("AAPL", 24)
        .await
        .expect_err("subscription denied must error");
    match err {
        NewsError::NoSubscription { provider_code } => {
            assert_eq!(provider_code, "DJ-N");
        }
        other => panic!("expected NoSubscription, got {other:?}"),
    }
}

#[tokio::test]
async fn news_providers_failure_propagates_as_typed_error() {
    let fake = Arc::new(FixtureIbkrNewsClient::with_phase6_aapl());
    fake.fail_news_providers(NewsError::NotConnected);
    let provider = IbkrNewsProvider::new(fake, limiter());

    let err = provider.fetch("AAPL", 24).await.expect_err("must error");
    assert!(matches!(err, NewsError::NotConnected));
}

#[tokio::test]
async fn provider_directory_cached_after_first_fetch() {
    // We can't observe the call count on the trait directly, but we
    // can prove cache behaviour by flipping `fail_news_providers` AFTER
    // the first successful fetch — a re-fetch must NOT see the new
    // forced error because the directory has been cached.
    let fake = Arc::new(FixtureIbkrNewsClient::with_phase6_aapl());
    let client: Arc<dyn IbkrNewsClient> = fake.clone();
    let provider = IbkrNewsProvider::new(client, limiter());

    provider.fetch("AAPL", 24).await.expect("warm-up ok");
    fake.fail_news_providers(NewsError::Other("should not be reached".to_string()));
    let items = provider
        .fetch("AAPL", 24)
        .await
        .expect("cached directory keeps subsequent fetches alive");
    assert!(!items.is_empty());
}
