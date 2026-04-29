//! Table-driven unit tests for `ParabolicShortDetector`.
//!
//! Each test builds a synthetic OHLCV fixture and invokes `evaluate`. Detector
//! is pure-functional; all that's needed is `tokio::test` for the async signature.

use chrono::Utc;

use crate::ibkr::types::{HistoricalBar, StrategyTag};
use crate::strategies::trait_def::DetectorError;
use crate::strategies::{Direction, MarketContext, StrategyDetector};

use super::detector::ParabolicShortDetector;

const SYMBOL: &str = "AAPL";

/// Symmetric H/L bar: high = close + 0.5, low = close - 0.5.
fn flat_bar(close: f64) -> HistoricalBar {
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

/// Up-day bar: open = prior_close, close = `close`, high = close+0.5, low = open-0.5.
fn up_bar(prior_close: f64, close: f64) -> HistoricalBar {
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

/// Apply per-day percentage gains in sequence, starting from `start`.
fn apply_pct_chain(start: f64, pcts: &[f64]) -> Vec<f64> {
    let mut out = Vec::with_capacity(pcts.len());
    let mut cur = start;
    for &p in pcts {
        cur *= 1.0 + p;
        out.push(cur);
    }
    out
}

/// Build the canonical "flat baseline + blow-off" daily fixture.
///
/// 25 bars total: `bars[0..21]` flat at 40 (low volatility, no losses → RSI = 100
/// after blow-off), then 4 strong consecutive up bars per `pcts`. The streak
/// breaks at `bars[20]` because `bars[20].close == bars[19].close == 40` (strict
/// `>` is required), so the consec count is exactly `pcts.len()`.
fn daily_blowoff(pcts: &[f64]) -> Vec<HistoricalBar> {
    let mut bars = Vec::with_capacity(21 + pcts.len());
    for _ in 0..21 {
        bars.push(flat_bar(40.0));
    }
    let mut prior = 40.0;
    for c in apply_pct_chain(40.0, pcts) {
        bars.push(up_bar(prior, c));
        prior = c;
    }
    bars
}

/// Build intraday 15-min bars from `(open, high, low, close)` tuples.
fn intraday_bars(rows: &[(f64, f64, f64, f64)]) -> Vec<HistoricalBar> {
    rows.iter()
        .enumerate()
        .map(|(i, &(o, h, l, c))| {
            let minute = 30 + (i as u32) * 15;
            let hh = 9 + minute / 60;
            let mm = minute % 60;
            HistoricalBar {
                time: format!("20240115 {hh:02}:{mm:02}:00"),
                open: o,
                high: h,
                low: l,
                close: c,
                volume: 250_000,
                wap: c,
                count: 0,
            }
        })
        .collect()
}

fn ctx<'a>(daily: &'a [HistoricalBar], intraday: Option<&'a [HistoricalBar]>) -> MarketContext<'a> {
    MarketContext {
        symbol: SYMBOL,
        daily_bars: daily,
        intraday_bars: intraday,
        fundamentals: None,
        recent_news: &[],
        news_verdict: None,
        current_quote: None,
        now: Utc::now(),
    }
}

#[tokio::test]
async fn fires_on_classic_blow_off_with_first_red_15m() {
    // 4 strong up days, ~53% cumulative, all per-day ≥ 5%.
    let daily = daily_blowoff(&[0.10, 0.12, 0.08, 0.15]);
    let today_close = daily.last().unwrap().close;

    // Intraday: green, green, RED (close < open), green. Trigger at the red close.
    let intraday = intraday_bars(&[
        (
            today_close,
            today_close + 1.5,
            today_close - 0.2,
            today_close + 1.0,
        ),
        (
            today_close + 1.0,
            today_close + 2.5,
            today_close + 0.5,
            today_close + 2.0,
        ),
        (
            today_close + 2.0,
            today_close + 2.4,
            today_close + 1.0,
            today_close + 1.2,
        ), // red
        (
            today_close + 1.2,
            today_close + 1.5,
            today_close + 0.5,
            today_close + 0.8,
        ),
    ]);

    let cand = ParabolicShortDetector::default()
        .evaluate(&ctx(&daily, Some(&intraday)))
        .await
        .expect("evaluate")
        .expect("expected setup candidate");

    assert_eq!(cand.strategy, "parabolic_short");
    assert_eq!(cand.tag, StrategyTag::ParabolicShort);
    assert_eq!(cand.direction, Direction::Short);
    let expected_trigger = today_close + 1.2;
    assert!(
        (cand.trigger_price - expected_trigger).abs() < 1e-9,
        "expected trigger {expected_trigger}, got {}",
        cand.trigger_price
    );
    assert!(
        cand.stop_price > cand.trigger_price,
        "short stop must be above trigger: stop={}, trigger={}",
        cand.stop_price,
        cand.trigger_price
    );
    assert_eq!(cand.targets.len(), 2);
    assert_eq!(cand.targets[0].label, "2R");
    assert_eq!(cand.targets[1].label, "3R");
}

#[tokio::test]
async fn does_not_fire_without_first_red_bar() {
    let daily = daily_blowoff(&[0.10, 0.12, 0.08, 0.15]);
    let today_close = daily.last().unwrap().close;

    // All intraday bars are green (close >= open).
    let intraday = intraday_bars(&[
        (
            today_close,
            today_close + 1.5,
            today_close - 0.2,
            today_close + 1.0,
        ),
        (
            today_close + 1.0,
            today_close + 2.5,
            today_close + 0.5,
            today_close + 2.0,
        ),
        (
            today_close + 2.0,
            today_close + 3.0,
            today_close + 1.5,
            today_close + 2.5,
        ),
    ]);

    let res = ParabolicShortDetector::default()
        .evaluate(&ctx(&daily, Some(&intraday)))
        .await
        .expect("evaluate");
    assert!(res.is_none(), "should not fire without a red 15-min bar");
}

#[tokio::test]
async fn does_not_fire_below_consec_minimum() {
    // Only 2 consecutive up days (per-day ≥ 5% but consec < 3).
    let daily = daily_blowoff(&[0.10, 0.12]);
    let today_close = daily.last().unwrap().close;

    let intraday = intraday_bars(&[(
        today_close,
        today_close + 1.0,
        today_close - 0.5,
        today_close - 0.3, // red
    )]);

    let res = ParabolicShortDetector::default()
        .evaluate(&ctx(&daily, Some(&intraday)))
        .await
        .expect("evaluate");
    assert!(res.is_none(), "should not fire below 3 consec up days");
}

#[tokio::test]
async fn does_not_fire_below_per_day_minimum() {
    // 4 consec up days but one is only +3% (below 5% floor). Cumulative still
    // qualifies (1.20*1.20*1.03*1.20 ≈ 1.78 → 78%) so the per-day gate is the
    // sole reason for rejection.
    let daily = daily_blowoff(&[0.20, 0.20, 0.03, 0.20]);
    let today_close = daily.last().unwrap().close;

    let intraday = intraday_bars(&[(
        today_close,
        today_close + 1.0,
        today_close - 0.5,
        today_close - 0.3, // red
    )]);

    let res = ParabolicShortDetector::default()
        .evaluate(&ctx(&daily, Some(&intraday)))
        .await
        .expect("evaluate");
    assert!(res.is_none(), "should not fire when any day is below 5%");
}

#[tokio::test]
async fn does_not_fire_below_cumulative_move() {
    // 5 consecutive up days, each at exactly 5%. Cumulative = 1.05^5 - 1 ≈ 27.6%
    // → below 40% threshold. Per-day gate passes (≥ 5%), so cumulative is the
    // discriminating filter.
    let daily = daily_blowoff(&[0.05, 0.05, 0.05, 0.05, 0.05]);
    let today_close = daily.last().unwrap().close;

    let intraday = intraday_bars(&[(
        today_close,
        today_close + 1.0,
        today_close - 0.5,
        today_close - 0.3, // red
    )]);

    let res = ParabolicShortDetector::default()
        .evaluate(&ctx(&daily, Some(&intraday)))
        .await
        .expect("evaluate");
    assert!(
        res.is_none(),
        "should not fire when cumulative move is below 40%"
    );
}

#[tokio::test]
async fn does_not_fire_when_not_extended_above_ma() {
    // High-volatility alternating baseline drives MA(20) and ATR(20) to large
    // values. Even a 4-bar blow-off keeps `(close - ma_20) / atr_20 < 2`.
    let mut bars = Vec::with_capacity(25);
    for i in 0..21 {
        let close = if i % 2 == 0 { 50.0 } else { 100.0 };
        bars.push(HistoricalBar {
            time: "20240101".into(),
            open: close,
            high: close + 0.5,
            low: close - 0.5,
            close,
            volume: 1_000_000,
            wap: close,
            count: 0,
        });
    }
    // bars[20] = 50 (even). For consec to fire, bars[21..25] must all be up vs prior.
    // From 50 → 55 (+10%) → 61.6 (+12%) → 66.53 (+8%) → 76.51 (+15%).
    let pcts = [0.10, 0.12, 0.08, 0.15];
    let mut prior = 50.0;
    for c in apply_pct_chain(50.0, &pcts) {
        bars.push(up_bar(prior, c));
        prior = c;
    }
    let today_close = bars.last().unwrap().close;

    let intraday = intraday_bars(&[(
        today_close,
        today_close + 1.0,
        today_close - 0.5,
        today_close - 0.3, // red
    )]);

    let res = ParabolicShortDetector::default()
        .evaluate(&ctx(&bars, Some(&intraday)))
        .await
        .expect("evaluate");
    assert!(
        res.is_none(),
        "should not fire when price is within 2 ATR of MA(20)"
    );
}

#[tokio::test]
async fn does_not_fire_with_low_rsi() {
    // Alternating big swings in the seed window keep RSI(14) below 80 even
    // after the 4-day blow-off. Distance check still passes (so RSI is the
    // discriminating filter). We use a tighter alternation to avoid driving
    // ATR so high that the distance check fails first.
    let mut bars = Vec::with_capacity(25);
    // 15 alternating bars for the RSI seed window: closes [50, 45, 50, 45, ...].
    for i in 0..15 {
        let close = if i % 2 == 0 { 50.0 } else { 45.0 };
        bars.push(HistoricalBar {
            time: "20240101".into(),
            open: close,
            high: close + 0.25,
            low: close - 0.25,
            close,
            volume: 1_000_000,
            wap: close,
            count: 0,
        });
    }
    // Then 6 flat bars at 50 to settle ATR back down.
    for _ in 0..6 {
        bars.push(flat_bar(50.0));
    }
    // bars[20] = 50, then 4 strong ups.
    let pcts = [0.10, 0.12, 0.08, 0.15];
    let mut prior = 50.0;
    for c in apply_pct_chain(50.0, &pcts) {
        bars.push(up_bar(prior, c));
        prior = c;
    }
    let today_close = bars.last().unwrap().close;

    let intraday = intraday_bars(&[(
        today_close,
        today_close + 1.0,
        today_close - 0.5,
        today_close - 0.3, // red
    )]);

    let res = ParabolicShortDetector::default()
        .evaluate(&ctx(&bars, Some(&intraday)))
        .await
        .expect("evaluate");
    assert!(res.is_none(), "should not fire when RSI(14) is below 80");
}

#[tokio::test]
async fn stop_is_session_high() {
    let daily = daily_blowoff(&[0.10, 0.12, 0.08, 0.15]);
    let today_close = daily.last().unwrap().close;

    // Highest high across the session is in bar 1.
    let session_high = today_close + 5.0;
    let intraday = intraday_bars(&[
        (
            today_close,
            session_high - 1.0,
            today_close - 0.2,
            today_close + 1.0,
        ),
        (
            today_close + 1.0,
            session_high, // peak
            today_close + 0.5,
            today_close + 2.0,
        ),
        (
            today_close + 2.0,
            today_close + 2.4,
            today_close + 1.0,
            today_close + 1.2,
        ), // red
        (
            today_close + 1.2,
            today_close + 1.5,
            today_close + 0.5,
            today_close + 0.8,
        ),
    ]);

    let cand = ParabolicShortDetector::default()
        .evaluate(&ctx(&daily, Some(&intraday)))
        .await
        .expect("evaluate")
        .expect("candidate");

    assert!(
        (cand.stop_price - session_high).abs() < 1e-9,
        "expected stop = session high {session_high}, got {}",
        cand.stop_price
    );
}

#[tokio::test]
async fn raw_signals_includes_consec_days_cumulative_move_atr_distance_rsi() {
    let daily = daily_blowoff(&[0.10, 0.12, 0.08, 0.15]);
    let today_close = daily.last().unwrap().close;

    let intraday = intraday_bars(&[
        (
            today_close,
            today_close + 1.0,
            today_close - 0.2,
            today_close + 0.5,
        ),
        (
            today_close + 0.5,
            today_close + 1.2,
            today_close + 0.0,
            today_close - 0.3, // red
        ),
    ]);

    let cand = ParabolicShortDetector::default()
        .evaluate(&ctx(&daily, Some(&intraday)))
        .await
        .expect("evaluate")
        .expect("candidate");

    let sigs = &cand.raw_signals;
    for key in ["consec_days", "cumulative_move", "atr_distance", "rsi_14"] {
        assert!(
            sigs.get(key).is_some(),
            "raw_signals missing '{key}': {sigs}"
        );
    }
}

#[tokio::test]
async fn targets_are_2r_3r_below_trigger_for_short() {
    // Engineer trigger=100 and stop=104 by choosing the red bar's close and
    // session high directly. Daily fixture stays canonical, but the intraday
    // open/highs are chosen so trigger=100, session_high=104.
    let daily = daily_blowoff(&[0.10, 0.12, 0.08, 0.15]);

    let intraday = intraday_bars(&[
        (101.0, 104.0, 100.5, 102.0), // green, session high = 104
        (102.0, 103.0, 99.5, 100.0),  // red close = 100
    ]);

    let cand = ParabolicShortDetector::default()
        .evaluate(&ctx(&daily, Some(&intraday)))
        .await
        .expect("evaluate")
        .expect("candidate");

    assert!((cand.trigger_price - 100.0).abs() < 1e-9);
    assert!((cand.stop_price - 104.0).abs() < 1e-9);
    assert!((cand.targets[0].price - 92.0).abs() < 1e-9);
    assert!((cand.targets[1].price - 88.0).abs() < 1e-9);
}

#[tokio::test]
async fn requires_intraday_bars() {
    let daily = daily_blowoff(&[0.10, 0.12, 0.08, 0.15]);

    let err = ParabolicShortDetector::default()
        .evaluate(&ctx(&daily, None))
        .await
        .expect_err("expected error");
    assert!(matches!(err, DetectorError::IntradayBarsRequired));
}
