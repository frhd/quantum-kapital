//! Table-driven unit tests for `EpisodicPivotDetector`.

use chrono::{Duration, Utc};

use crate::ibkr::types::{HistoricalBar, NewsItem, TickerSentiment};
use crate::strategies::trait_def::DetectorError;
use crate::strategies::{Direction, MarketContext, StrategyDetector};

use super::detector::EpisodicPivotDetector;

const SYMBOL: &str = "AAPL";

fn bar(open: f64, high: f64, low: f64, close: f64, volume: i64) -> HistoricalBar {
    HistoricalBar {
        time: "20240115".into(),
        open,
        high,
        low,
        close,
        volume,
        wap: close,
        count: 0,
    }
}

/// Build a 5-day daily series. Days 0..3 are baseline, day 3 is "yesterday"
/// with the supplied close + volume, day 4 is "today" with the supplied open.
fn daily_series(
    yesterday_close: f64,
    today_open: f64,
    yesterday_volume: i64,
) -> Vec<HistoricalBar> {
    vec![
        bar(50.0, 50.5, 49.5, 50.0, 800_000),
        bar(50.0, 50.5, 49.5, 50.0, 800_000),
        bar(50.0, 50.5, 49.5, 50.0, 800_000),
        bar(
            yesterday_close - 0.5,
            yesterday_close + 0.5,
            yesterday_close - 0.5,
            yesterday_close,
            yesterday_volume,
        ),
        bar(
            today_open,
            today_open + 0.5,
            today_open - 0.5,
            today_open,
            500_000,
        ),
    ]
}

/// Build intraday 15-min bars. `volumes[i]` and `highs[i]` parameterize bar `i`.
fn intraday_series(volumes: &[i64], highs: &[f64]) -> Vec<HistoricalBar> {
    volumes
        .iter()
        .zip(highs.iter())
        .enumerate()
        .map(|(i, (&v, &h))| {
            let minute = 30 + (i as u32) * 15;
            let hh = 9 + minute / 60;
            let mm = minute % 60;
            HistoricalBar {
                time: format!("20240115 {hh:02}:{mm:02}:00"),
                open: 50.0,
                high: h,
                low: 49.0,
                close: 50.0,
                volume: v,
                wap: 50.0,
                count: 0,
            }
        })
        .collect()
}

fn news_with_sentiment(symbol: &str, score: f64, relevance: f64) -> NewsItem {
    let label = if score > 0.0 { "Bullish" } else { "Bearish" };
    NewsItem {
        time_published: Utc::now() - Duration::hours(2),
        title: "test headline".into(),
        summary: "test summary".into(),
        source: "test source".into(),
        url: "https://example.test/article".into(),
        overall_sentiment_score: Some(score),
        overall_sentiment_label: Some(label.into()),
        ticker_sentiment: vec![TickerSentiment {
            ticker: symbol.to_string(),
            relevance_score: relevance,
            ticker_sentiment_score: score,
            ticker_sentiment_label: label.into(),
        }],
    }
}

fn ctx<'a>(
    symbol: &'a str,
    daily: &'a [HistoricalBar],
    intraday: Option<&'a [HistoricalBar]>,
    news: &'a [NewsItem],
) -> MarketContext<'a> {
    MarketContext {
        symbol,
        daily_bars: daily,
        intraday_bars: intraday,
        fundamentals: None,
        recent_news: news,
        current_quote: None,
        now: Utc::now(),
    }
}

#[tokio::test]
async fn fires_long_on_gap_up_with_bullish_news() {
    let daily = daily_series(50.0, 53.0, 1_000_000); // +6% gap
    let intraday = intraday_series(&[600_000, 600_000, 200_000], &[53.4, 53.6, 53.5]);
    let news = vec![news_with_sentiment(SYMBOL, 0.4, 0.8)];

    let cand = EpisodicPivotDetector
        .evaluate(&ctx(SYMBOL, &daily, Some(&intraday), &news))
        .await
        .expect("evaluate")
        .expect("expected candidate");

    assert_eq!(cand.strategy, "episodic_pivot");
    assert_eq!(cand.direction, Direction::Long);
    assert!((cand.trigger_price - 53.0).abs() < 1e-9);
    assert!(cand.stop_price < cand.trigger_price);
}

#[tokio::test]
async fn fires_short_on_gap_up_with_bearish_news() {
    let daily = daily_series(50.0, 53.0, 1_000_000); // +6% gap
    let intraday = intraday_series(&[700_000, 500_000, 100_000], &[53.4, 53.8, 53.5]);
    let news = vec![news_with_sentiment(SYMBOL, -0.3, 0.8)];

    let cand = EpisodicPivotDetector
        .evaluate(&ctx(SYMBOL, &daily, Some(&intraday), &news))
        .await
        .expect("evaluate")
        .expect("expected fade-short candidate");

    assert_eq!(cand.direction, Direction::Short);
    assert!(cand.stop_price > cand.trigger_price);
}

#[tokio::test]
async fn fires_short_on_gap_down_with_bearish_news() {
    let daily = daily_series(50.0, 47.5, 1_000_000); // -5% gap
    let intraday = intraday_series(&[600_000, 600_000, 200_000], &[47.6, 47.9, 47.8]);
    let news = vec![news_with_sentiment(SYMBOL, -0.4, 0.8)];

    let cand = EpisodicPivotDetector
        .evaluate(&ctx(SYMBOL, &daily, Some(&intraday), &news))
        .await
        .expect("evaluate")
        .expect("expected short candidate");

    assert_eq!(cand.direction, Direction::Short);
    assert!(cand.stop_price > cand.trigger_price);
}

#[tokio::test]
async fn does_not_fire_without_news() {
    let daily = daily_series(50.0, 53.0, 1_000_000);
    let intraday = intraday_series(&[600_000, 600_000], &[53.4, 53.6]);

    let res = EpisodicPivotDetector
        .evaluate(&ctx(SYMBOL, &daily, Some(&intraday), &[]))
        .await
        .expect("evaluate");
    assert!(res.is_none());
}

#[tokio::test]
async fn does_not_fire_with_neutral_sentiment() {
    let daily = daily_series(50.0, 53.0, 1_000_000);
    let intraday = intraday_series(&[600_000, 600_000], &[53.4, 53.6]);
    let news = vec![news_with_sentiment(SYMBOL, 0.10, 0.8)]; // within ±0.15

    let res = EpisodicPivotDetector
        .evaluate(&ctx(SYMBOL, &daily, Some(&intraday), &news))
        .await
        .expect("evaluate");
    assert!(res.is_none());
}

#[tokio::test]
async fn does_not_fire_below_min_gap() {
    let daily = daily_series(50.0, 51.0, 1_000_000); // +2% gap (below 4%)
    let intraday = intraday_series(&[600_000, 600_000], &[51.2, 51.3]);
    let news = vec![news_with_sentiment(SYMBOL, 0.4, 0.8)];

    let res = EpisodicPivotDetector
        .evaluate(&ctx(SYMBOL, &daily, Some(&intraday), &news))
        .await
        .expect("evaluate");
    assert!(res.is_none());
}

#[tokio::test]
async fn does_not_fire_without_volume_confirmation() {
    let daily = daily_series(50.0, 53.0, 1_000_000);
    // first 30 min volume = 100_000 + 100_000 = 200_000 < prior_day 1_000_000
    let intraday = intraday_series(&[100_000, 100_000, 100_000], &[53.4, 53.6, 53.5]);
    let news = vec![news_with_sentiment(SYMBOL, 0.4, 0.8)];

    let res = EpisodicPivotDetector
        .evaluate(&ctx(SYMBOL, &daily, Some(&intraday), &news))
        .await
        .expect("evaluate");
    assert!(res.is_none());
}

#[tokio::test]
async fn requires_intraday_bars() {
    let daily = daily_series(50.0, 53.0, 1_000_000); // gap qualifies
    let news = vec![news_with_sentiment(SYMBOL, 0.4, 0.8)];

    let err = EpisodicPivotDetector
        .evaluate(&ctx(SYMBOL, &daily, None, &news))
        .await
        .expect_err("expected error");
    assert!(matches!(err, DetectorError::IntradayBarsRequired));
}

#[tokio::test]
async fn stop_for_long_is_pre_gap_close() {
    let daily = daily_series(50.0, 53.0, 1_000_000);
    let intraday = intraday_series(&[600_000, 600_000], &[53.4, 53.6]);
    let news = vec![news_with_sentiment(SYMBOL, 0.4, 0.8)];

    let cand = EpisodicPivotDetector
        .evaluate(&ctx(SYMBOL, &daily, Some(&intraday), &news))
        .await
        .expect("evaluate")
        .expect("candidate");

    assert_eq!(cand.direction, Direction::Long);
    assert!(
        (cand.stop_price - 50.0).abs() < 1e-9,
        "expected stop at pre-gap close 50.0, got {}",
        cand.stop_price
    );
}

#[tokio::test]
async fn stop_for_short_is_gap_day_high() {
    let daily = daily_series(50.0, 47.5, 1_000_000);
    let intraday = intraday_series(&[600_000, 600_000, 200_000], &[47.7, 48.1, 47.9]);
    let news = vec![news_with_sentiment(SYMBOL, -0.4, 0.8)];

    let cand = EpisodicPivotDetector
        .evaluate(&ctx(SYMBOL, &daily, Some(&intraday), &news))
        .await
        .expect("evaluate")
        .expect("candidate");

    assert_eq!(cand.direction, Direction::Short);
    assert!(
        (cand.stop_price - 48.1).abs() < 1e-9,
        "expected stop at intraday high 48.1, got {}",
        cand.stop_price
    );
}

#[tokio::test]
async fn raw_signals_includes_gap_pct_sentiment_volume_ratio() {
    let daily = daily_series(50.0, 53.0, 1_000_000);
    let intraday = intraday_series(&[600_000, 600_000], &[53.4, 53.6]);
    let news = vec![news_with_sentiment(SYMBOL, 0.4, 0.8)];

    let cand = EpisodicPivotDetector
        .evaluate(&ctx(SYMBOL, &daily, Some(&intraday), &news))
        .await
        .expect("evaluate")
        .expect("candidate");

    let sigs = &cand.raw_signals;
    for key in ["gap_pct", "sentiment_score", "volume_ratio"] {
        assert!(
            sigs.get(key).is_some(),
            "raw_signals missing '{key}': {sigs}"
        );
    }
}

#[tokio::test]
async fn most_relevant_news_item_drives_sentiment() {
    let daily = daily_series(50.0, 53.0, 1_000_000); // gap up
    let intraday = intraday_series(&[600_000, 600_000], &[53.4, 53.6]);
    // Bullish item with low relevance + bearish item with high relevance.
    // The bearish (more relevant) one must win → fade short for gap up.
    let news = vec![
        news_with_sentiment(SYMBOL, 0.4, 0.3),
        news_with_sentiment(SYMBOL, -0.3, 0.9),
    ];

    let cand = EpisodicPivotDetector
        .evaluate(&ctx(SYMBOL, &daily, Some(&intraday), &news))
        .await
        .expect("evaluate")
        .expect("candidate");

    assert_eq!(
        cand.direction,
        Direction::Short,
        "more-relevant bearish item should override bullish lower-relevance item"
    );
}
