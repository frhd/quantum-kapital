//! Phase 22 — `DetectorsConfig` round-trip and threshold-override tests.
//!
//! Verifies that:
//! 1. `DetectorsConfig::default()` mirrors the Phase 07–09 baselines.
//! 2. Each detector honors its configured thresholds (volume_multiple,
//!    min_gap_pct, min_consec_days) instead of the old hardcoded constants.
//! 3. `AppConfig` serializes / deserializes through `serde_json` cleanly.
//! 4. A settings file written before this phase (no `detectors` block)
//!    deserializes with all fields defaulted.

use chrono::Utc;

use crate::config::AppConfig;
use crate::ibkr::types::{HistoricalBar, NewsItem, TickerSentiment};
use crate::strategies::breakout::BreakoutDetector;
use crate::strategies::episodic_pivot::EpisodicPivotDetector;
use crate::strategies::parabolic_short::ParabolicShortDetector;
use crate::strategies::{
    BreakoutCfg, DetectorsConfig, EpisodicPivotCfg, MarketContext, ParabolicShortCfg,
    StrategyDetector,
};

// ---------- defaults regression guard ----------

#[test]
fn default_detector_config_matches_phase_07_through_09_constants() {
    let cfg = DetectorsConfig::default();

    assert_eq!(cfg.breakout.lookback_days, 20);
    assert!((cfg.breakout.volume_multiple - 1.5).abs() < 1e-9);
    assert!((cfg.breakout.rsi_ceiling - 80.0).abs() < 1e-9);
    assert_eq!(cfg.breakout.atr_period, 14);
    assert_eq!(cfg.breakout.swing_low_period, 10);

    assert!((cfg.episodic_pivot.min_gap_pct - 0.04).abs() < 1e-9);
    assert!((cfg.episodic_pivot.min_sentiment_abs - 0.15).abs() < 1e-9);
    assert!((cfg.episodic_pivot.min_volume_ratio - 1.0).abs() < 1e-9);

    assert_eq!(cfg.parabolic_short.min_consec_days, 3);
    assert!((cfg.parabolic_short.min_per_day_move - 0.05).abs() < 1e-9);
    assert!((cfg.parabolic_short.min_cumulative_move - 0.40).abs() < 1e-9);
    assert!((cfg.parabolic_short.min_atr_distance - 2.0).abs() < 1e-9);
    assert!((cfg.parabolic_short.min_rsi - 80.0).abs() < 1e-9);
}

// ---------- breakout: configurable volume multiple ----------

fn make_bar(close: f64, volume: i64, high: f64, low: f64) -> HistoricalBar {
    HistoricalBar {
        time: "20240101".into(),
        open: close,
        high,
        low,
        close,
        volume,
        wap: close,
        count: 0,
    }
}
fn sym_bar(close: f64, volume: i64, half_spread: f64) -> HistoricalBar {
    make_bar(close, volume, close + half_spread, close - half_spread)
}
/// Mirror of the breakout test fixture: 25-bar warm-up, 9-bar consolidation,
/// then today. The prior-window mean volume is 1_000_000, so
/// `today_volume / 1e6` equals the volume multiple.
fn breakout_fixture(today_close: f64, today_volume: i64) -> Vec<HistoricalBar> {
    let mut bars = Vec::with_capacity(35);
    let warmup = [0.3_f64, -0.2];
    let mut close = 50.0;
    for i in 0..25 {
        close += warmup[i % 2];
        bars.push(make_bar(close, 1_000_000, close + 0.5, close - 0.5));
    }
    let consol = [0.05_f64, -0.03];
    for i in 0..9 {
        close += consol[i % 2];
        bars.push(sym_bar(close, 1_000_000, 0.025));
    }
    bars.push(sym_bar(today_close, today_volume, 0.025));
    bars
}
fn breakout_ctx<'a>(symbol: &'a str, bars: &'a [HistoricalBar]) -> MarketContext<'a> {
    MarketContext {
        symbol,
        daily_bars: bars,
        intraday_bars: None,
        fundamentals: None,
        recent_news: &[],
        news_verdict: None,
        current_quote: None,
        now: Utc::now(),
    }
}

#[tokio::test]
async fn breakout_detector_uses_configured_volume_multiple() {
    // Vol mult ≈ 1.7× — passes default (1.5×) but must fail at 2.0×.
    let bars = breakout_fixture(52.0, 1_700_000);

    let default_detector = BreakoutDetector::default();
    let cand = default_detector
        .evaluate(&breakout_ctx("AAPL", &bars))
        .await
        .expect("evaluate")
        .expect("default 1.5× threshold should fire at 1.7×");
    assert_eq!(cand.strategy, "breakout");

    let cfg = BreakoutCfg {
        volume_multiple: 2.0,
        ..BreakoutCfg::default()
    };
    let strict = BreakoutDetector::with_config(cfg);
    let res = strict
        .evaluate(&breakout_ctx("AAPL", &bars))
        .await
        .expect("evaluate");
    assert!(
        res.is_none(),
        "configured 2.0× threshold must reject 1.7× volume"
    );
}

// ---------- episodic_pivot: configurable min_gap_pct ----------

fn ep_bar(open: f64, high: f64, low: f64, close: f64, volume: i64) -> HistoricalBar {
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
fn ep_daily_series(
    yesterday_close: f64,
    today_open: f64,
    yesterday_volume: i64,
) -> Vec<HistoricalBar> {
    vec![
        ep_bar(50.0, 50.5, 49.5, 50.0, 800_000),
        ep_bar(50.0, 50.5, 49.5, 50.0, 800_000),
        ep_bar(50.0, 50.5, 49.5, 50.0, 800_000),
        ep_bar(
            yesterday_close - 0.5,
            yesterday_close + 0.5,
            yesterday_close - 0.5,
            yesterday_close,
            yesterday_volume,
        ),
        ep_bar(
            today_open,
            today_open + 0.5,
            today_open - 0.5,
            today_open,
            500_000,
        ),
    ]
}
fn ep_intraday(volumes: &[i64], highs: &[f64]) -> Vec<HistoricalBar> {
    volumes
        .iter()
        .zip(highs.iter())
        .map(|(&v, &h)| HistoricalBar {
            time: "20240115 09:30:00".into(),
            open: 50.0,
            high: h,
            low: 49.0,
            close: 50.0,
            volume: v,
            wap: 50.0,
            count: 0,
        })
        .collect()
}
fn ep_news(symbol: &str, score: f64, relevance: f64) -> NewsItem {
    let label = if score > 0.0 { "Bullish" } else { "Bearish" };
    NewsItem {
        time_published: Utc::now(),
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
fn ep_ctx<'a>(
    symbol: &'a str,
    daily: &'a [HistoricalBar],
    intraday: &'a [HistoricalBar],
    news: &'a [NewsItem],
) -> MarketContext<'a> {
    MarketContext {
        symbol,
        daily_bars: daily,
        intraday_bars: Some(intraday),
        fundamentals: None,
        recent_news: news,
        news_verdict: None,
        current_quote: None,
        now: Utc::now(),
    }
}

#[tokio::test]
async fn episodic_pivot_detector_uses_configured_min_gap_pct() {
    // Yesterday close 50, today open 52.5 → 5% gap up. Default (4%) fires;
    // configured 6% threshold must reject.
    let daily = ep_daily_series(50.0, 52.5, 1_000_000);
    let intraday = ep_intraday(&[600_000, 600_000], &[52.7, 52.9]);
    let news = vec![ep_news("AAPL", 0.4, 0.8)];

    let default_detector = EpisodicPivotDetector::default();
    default_detector
        .evaluate(&ep_ctx("AAPL", &daily, &intraday, &news))
        .await
        .expect("evaluate")
        .expect("default 4% gap threshold should fire on 5% gap");

    let cfg = EpisodicPivotCfg {
        min_gap_pct: 0.06,
        ..EpisodicPivotCfg::default()
    };
    let strict = EpisodicPivotDetector::with_config(cfg);
    let res = strict
        .evaluate(&ep_ctx("AAPL", &daily, &intraday, &news))
        .await
        .expect("evaluate");
    assert!(
        res.is_none(),
        "configured 6% gap threshold must reject 5% gap"
    );
}

// ---------- parabolic_short: configurable min_consec_days ----------

fn ps_flat_bar(close: f64) -> HistoricalBar {
    HistoricalBar {
        time: "20240101".into(),
        open: close,
        high: close + 0.5,
        low: close - 0.5,
        close,
        volume: 1_000_000,
        wap: close,
        count: 0,
    }
}
fn ps_up_bar(prior_close: f64, close: f64) -> HistoricalBar {
    HistoricalBar {
        time: "20240101".into(),
        open: prior_close,
        high: close + 0.5,
        low: prior_close - 0.5,
        close,
        volume: 2_000_000,
        wap: close,
        count: 0,
    }
}
fn ps_apply_pct_chain(start: f64, pcts: &[f64]) -> Vec<f64> {
    let mut out = Vec::with_capacity(pcts.len());
    let mut cur = start;
    for &p in pcts {
        cur *= 1.0 + p;
        out.push(cur);
    }
    out
}
fn ps_daily_blowoff(pcts: &[f64]) -> Vec<HistoricalBar> {
    let mut bars = Vec::with_capacity(21 + pcts.len());
    for _ in 0..21 {
        bars.push(ps_flat_bar(40.0));
    }
    let mut prior = 40.0;
    for c in ps_apply_pct_chain(40.0, pcts) {
        bars.push(ps_up_bar(prior, c));
        prior = c;
    }
    bars
}
fn ps_intraday(rows: &[(f64, f64, f64, f64)]) -> Vec<HistoricalBar> {
    rows.iter()
        .map(|&(o, h, l, c)| HistoricalBar {
            time: "20240115 09:30:00".into(),
            open: o,
            high: h,
            low: l,
            close: c,
            volume: 250_000,
            wap: c,
            count: 0,
        })
        .collect()
}
fn ps_ctx<'a>(daily: &'a [HistoricalBar], intraday: &'a [HistoricalBar]) -> MarketContext<'a> {
    MarketContext {
        symbol: "AAPL",
        daily_bars: daily,
        intraday_bars: Some(intraday),
        fundamentals: None,
        recent_news: &[],
        news_verdict: None,
        current_quote: None,
        now: Utc::now(),
    }
}

#[tokio::test]
async fn parabolic_short_detector_uses_configured_consec_days() {
    // 3 strong up days; cumulative ≈ 33% — below default 40%, so we use a
    // 4-day fixture and bump min_consec_days to 4 to test the gate. With
    // default min_consec_days=3, the 3-day prefix of the streak fires;
    // when we require 4, the same 3-day fixture must skip.
    let daily_3day = ps_daily_blowoff(&[0.18, 0.18, 0.18]);
    let today_close = daily_3day.last().unwrap().close;
    let intraday = ps_intraday(&[(
        today_close,
        today_close + 1.0,
        today_close - 0.5,
        today_close - 0.3,
    )]);

    let default_detector = ParabolicShortDetector::default();
    let cand = default_detector
        .evaluate(&ps_ctx(&daily_3day, &intraday))
        .await
        .expect("evaluate")
        .expect("default 3-day threshold should fire on 3-day streak");
    assert_eq!(cand.strategy, "parabolic_short");

    let cfg = ParabolicShortCfg {
        min_consec_days: 4,
        ..ParabolicShortCfg::default()
    };
    let strict = ParabolicShortDetector::with_config(cfg);
    let res = strict
        .evaluate(&ps_ctx(&daily_3day, &intraday))
        .await
        .expect("evaluate");
    assert!(
        res.is_none(),
        "configured 4-day threshold must reject 3-day streak"
    );
}

// ---------- AppConfig serialization ----------

#[test]
fn serializing_settings_round_trips() {
    let mut cfg = AppConfig::default();
    cfg.detectors.breakout.volume_multiple = 2.5;
    cfg.detectors.breakout.lookback_days = 30;
    cfg.detectors.episodic_pivot.min_gap_pct = 0.08;
    cfg.detectors.parabolic_short.min_consec_days = 4;
    cfg.detectors.parabolic_short.min_rsi = 85.0;

    let json = serde_json::to_string(&cfg).expect("serialize");
    let parsed: AppConfig = serde_json::from_str(&json).expect("deserialize");

    assert!((parsed.detectors.breakout.volume_multiple - 2.5).abs() < 1e-9);
    assert_eq!(parsed.detectors.breakout.lookback_days, 30);
    assert!((parsed.detectors.episodic_pivot.min_gap_pct - 0.08).abs() < 1e-9);
    assert_eq!(parsed.detectors.parabolic_short.min_consec_days, 4);
    assert!((parsed.detectors.parabolic_short.min_rsi - 85.0).abs() < 1e-9);
}

#[test]
fn missing_detector_section_falls_back_to_defaults() {
    // settings.json written before Phase 22: no `detectors` block.
    let json = r#"{
        "ibkr": {
            "default_host": "127.0.0.1",
            "default_port": 4004,
            "default_client_id": 100,
            "connection_timeout_ms": 30000,
            "reconnect_interval_ms": 5000,
            "max_reconnect_attempts": 3,
            "rate_limit_per_second": 50
        },
        "logging": {
            "level": "info",
            "file_path": null,
            "max_file_size_mb": 10,
            "max_files": 5,
            "console_output": true
        },
        "ui": {
            "theme": "dark",
            "default_refresh_interval_ms": 1000,
            "show_notifications": true,
            "auto_save_layout": true
        },
        "api": {
            "alpha_vantage_api_key": null
        }
    }"#;

    let parsed: AppConfig = serde_json::from_str(json).expect("deserialize legacy settings");
    assert_eq!(parsed.detectors.breakout.lookback_days, 20);
    assert!((parsed.detectors.breakout.volume_multiple - 1.5).abs() < 1e-9);
    assert_eq!(parsed.detectors.parabolic_short.min_consec_days, 3);
    assert!((parsed.detectors.episodic_pivot.min_gap_pct - 0.04).abs() < 1e-9);
}
