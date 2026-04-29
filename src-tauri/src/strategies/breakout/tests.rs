//! Table-driven unit tests for `BreakoutDetector`.
//!
//! Each test builds a synthetic OHLCV fixture and invokes `evaluate`. The
//! detector is pure-functional so we never need a runtime fixture beyond
//! `tokio::test` for the async signature.

use chrono::Utc;

use crate::ibkr::types::{HistoricalBar, StrategyTag};
use crate::strategies::trait_def::DetectorError;
use crate::strategies::{Direction, MarketContext, StrategyDetector};

use super::detector::BreakoutDetector;

fn ctx<'a>(symbol: &'a str, bars: &'a [HistoricalBar]) -> MarketContext<'a> {
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

/// Symmetric H/L bar around the close: low = close - half_spread, high = close + half_spread.
fn sym_bar(close: f64, volume: i64, half_spread: f64) -> HistoricalBar {
    make_bar(close, volume, close + half_spread, close - half_spread)
}

/// 35-bar fixture that mounts a clean breakout.
///
/// Layout:
/// - Bars 0..25: alternating [+0.3, -0.2] pattern with H-L = 1.0 (TR = 1.0 →
///   ATR ≈ 1.0). Steady-state RSI for this pattern is 60 (avg gain 0.15 vs
///   avg loss 0.10).
/// - Bars 25..34: tight consolidation, alternating [+0.05, -0.03] with
///   H-L = 0.05. Drags ATR down from 1.0 to ≈0.55 via Wilder smoothing.
/// - Bar 34: today (caller-supplied close + volume), H-L = 0.05.
///
/// Prior-20-day max close lands at `bars[33].close ≈ 51.63`. With RSI in the
/// 60–75 band and the wide warm-up keeping ATR ≈ 0.55, the recent tight
/// lows sit *above* `trigger − ATR` (case A for the swing-low stop test).
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

#[tokio::test]
async fn fires_on_new_20d_high_with_volume_confirmation() {
    let bars = breakout_fixture(52.0, 2_000_000);
    let detector = BreakoutDetector::default();

    let candidate = detector
        .evaluate(&ctx("AAPL", &bars))
        .await
        .expect("evaluate")
        .expect("expected setup candidate");

    assert_eq!(candidate.strategy, "breakout");
    assert_eq!(candidate.tag, StrategyTag::Breakout);
    assert_eq!(candidate.direction, Direction::Long);
    assert!((candidate.trigger_price - 52.0).abs() < 1e-9);
    assert!(candidate.stop_price < candidate.trigger_price);
    assert_eq!(candidate.targets.len(), 2);
    assert_eq!(candidate.targets[0].label, "2R");
    assert_eq!(candidate.targets[1].label, "3R");
}

#[tokio::test]
async fn does_not_fire_without_volume() {
    let bars = breakout_fixture(52.0, 700_000);
    let detector = BreakoutDetector::default();
    let result = detector
        .evaluate(&ctx("AAPL", &bars))
        .await
        .expect("evaluate");
    assert!(
        result.is_none(),
        "should not fire without volume confirmation"
    );
}

#[tokio::test]
async fn does_not_fire_when_not_a_new_high() {
    // Today's close ~= bars[33].close, no breakout
    let bars = breakout_fixture(51.0, 2_000_000);
    let detector = BreakoutDetector::default();
    let result = detector
        .evaluate(&ctx("AAPL", &bars))
        .await
        .expect("evaluate");
    assert!(result.is_none(), "should not fire when not at new high");
}

#[tokio::test]
async fn does_not_fire_when_rsi_above_80() {
    // Steep linear ramp → RSI = 100. Volume confirmed and new high.
    let mut bars: Vec<HistoricalBar> = (0..34)
        .map(|i| {
            make_bar(
                50.0 + 2.0 * i as f64,
                1_000_000,
                52.0 + 2.0 * i as f64,
                49.0 + 2.0 * i as f64,
            )
        })
        .collect();
    let last_close = bars.last().unwrap().close + 1.0;
    bars.push(make_bar(
        last_close,
        2_000_000,
        last_close + 0.5,
        last_close - 0.5,
    ));

    let detector = BreakoutDetector::default();
    let result = detector
        .evaluate(&ctx("AAPL", &bars))
        .await
        .expect("evaluate");
    assert!(
        result.is_none(),
        "RSI(14) is overextended (=100), must not fire; got {:?}",
        result
    );
}

#[tokio::test]
async fn requires_min_lookback() {
    let bars: Vec<HistoricalBar> = (0..15)
        .map(|i| sym_bar(50.0 + i as f64, 1_000_000, 0.5))
        .collect();
    let detector = BreakoutDetector::default();
    let err = detector
        .evaluate(&ctx("AAPL", &bars))
        .await
        .expect_err("expected error");
    assert!(matches!(err, DetectorError::InsufficientBars { .. }));
}

#[tokio::test]
async fn stop_uses_swing_low_when_tighter_than_atr_distance() {
    // breakout_fixture is designed so the recent 10 bars have very tight lows
    // (close − 0.05 each, climbing) and ATR is dominated by the wide warm-up
    // phase. swing_low_10 should land *above* trigger − ATR, so stop =
    // swing_low_10.
    let bars = breakout_fixture(52.0, 2_000_000);
    let detector = BreakoutDetector::default();
    let cand = detector
        .evaluate(&ctx("AAPL", &bars))
        .await
        .expect("evaluate")
        .expect("candidate");

    let swing_low_10 = bars[bars.len() - 10..]
        .iter()
        .map(|b| b.low)
        .fold(f64::INFINITY, f64::min);
    let atr_14 = cand
        .raw_signals
        .get("atr_14")
        .and_then(|v| v.as_f64())
        .expect("atr_14");

    assert!(
        swing_low_10 > cand.trigger_price - atr_14,
        "fixture invariant violated: swing_low {swing_low_10} not above trigger-ATR {}",
        cand.trigger_price - atr_14
    );
    assert!(
        (cand.stop_price - swing_low_10).abs() < 1e-6,
        "expected stop = swing_low {swing_low_10}, got {}",
        cand.stop_price
    );
}

#[tokio::test]
async fn stop_uses_atr_distance_when_swing_low_too_far() {
    // Take the firing fixture and inject a deep low into one of the recent 10
    // bars. swing_low_10 plummets below trigger − ATR; stop should clamp to
    // trigger − ATR.
    let mut bars = breakout_fixture(52.0, 2_000_000);
    let n = bars.len();
    let deep_idx = n - 6; // 5 days before today, well within the 10-bar swing window
    let close = bars[deep_idx].close;
    bars[deep_idx] = make_bar(close, 1_000_000, close + 0.05, 30.0);

    let detector = BreakoutDetector::default();
    let cand = detector
        .evaluate(&ctx("AAPL", &bars))
        .await
        .expect("evaluate")
        .expect("candidate");

    let atr_14 = cand
        .raw_signals
        .get("atr_14")
        .and_then(|v| v.as_f64())
        .expect("atr_14");
    let expected_stop = cand.trigger_price - atr_14;
    assert!(
        (cand.stop_price - expected_stop).abs() < 1e-6,
        "expected stop = trigger-ATR {expected_stop}, got {}",
        cand.stop_price
    );
}

#[tokio::test]
async fn targets_are_2r_and_3r_above_trigger_for_long() {
    let bars = breakout_fixture(52.0, 2_000_000);
    let detector = BreakoutDetector::default();
    let cand = detector
        .evaluate(&ctx("AAPL", &bars))
        .await
        .expect("evaluate")
        .expect("candidate");

    let risk = cand.trigger_price - cand.stop_price;
    assert!(risk > 0.0);
    assert!((cand.targets[0].price - (cand.trigger_price + 2.0 * risk)).abs() < 1e-9);
    assert!((cand.targets[1].price - (cand.trigger_price + 3.0 * risk)).abs() < 1e-9);
}

#[tokio::test]
async fn raw_signals_includes_required_keys() {
    let bars = breakout_fixture(52.0, 2_000_000);
    let detector = BreakoutDetector::default();
    let cand = detector
        .evaluate(&ctx("AAPL", &bars))
        .await
        .expect("evaluate")
        .expect("candidate");

    let signals = &cand.raw_signals;
    for key in [
        "lookback_high",
        "volume_multiple",
        "atr_14",
        "swing_low_10",
        "rsi_14",
    ] {
        assert!(
            signals.get(key).is_some(),
            "raw_signals missing '{key}': {signals}"
        );
    }
}

#[tokio::test]
async fn conviction_signal_scales_with_volume_multiple() {
    let detector = BreakoutDetector::default();

    let bars_15 = breakout_fixture(52.0, 1_500_000); // mult 1.5
    let bars_30 = breakout_fixture(52.0, 3_000_000); // mult 3.0

    let c1_5 = detector
        .evaluate(&ctx("AAPL", &bars_15))
        .await
        .expect("evaluate")
        .expect("candidate")
        .conviction_signal;
    let c3_0 = detector
        .evaluate(&ctx("AAPL", &bars_30))
        .await
        .expect("evaluate")
        .expect("candidate")
        .conviction_signal;

    assert!(
        (c1_5 - 0.5).abs() < 0.05,
        "expected ~0.5 at 1.5× vol, got {c1_5}"
    );
    assert!(
        (c3_0 - 0.85).abs() < 0.05,
        "expected ~0.85 at 3.0× vol, got {c3_0}"
    );
    assert!((0.0..=1.0).contains(&c1_5));
    assert!((0.0..=1.0).contains(&c3_0));
}

#[tokio::test]
async fn degenerate_zero_atr_does_not_panic() {
    // 35 perfectly flat bars (OHLC all = 100, vol = 1_000_000), then today
    // also flat but volume = 2× avg. ATR = 0, swing_low = close = trigger.
    let mut bars: Vec<HistoricalBar> = (0..34).map(|_| sym_bar(100.0, 1_000_000, 0.0)).collect();
    bars.push(sym_bar(100.0, 2_000_000, 0.0));

    let detector = BreakoutDetector::default();
    let res = detector.evaluate(&ctx("AAPL", &bars)).await;
    match res {
        Ok(None) => {}
        Ok(Some(_)) => panic!("flat-line bars should not produce a candidate"),
        Err(e) => panic!("flat-line bars must not error, got {e:?}"),
    }
}
