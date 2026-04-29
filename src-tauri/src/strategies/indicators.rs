//! Technical indicator helpers for strategy detectors.
//!
//! All functions are point-in-time: they consume a slice of bars (or closes)
//! and return a single scalar for the most recent observation. Out-of-range
//! inputs return `None` rather than panic.

use crate::ibkr::types::HistoricalBar;

/// Wilder's Average True Range over `period` bars.
///
/// Returns `None` if there are fewer than `period + 1` bars (need a prior
/// close for the first true range).
pub fn atr(bars: &[HistoricalBar], period: usize) -> Option<f64> {
    if period == 0 || bars.len() <= period {
        return None;
    }
    let trs: Vec<f64> = (1..bars.len())
        .map(|i| {
            let h = bars[i].high;
            let l = bars[i].low;
            let pc = bars[i - 1].close;
            (h - l).max((h - pc).abs()).max((l - pc).abs())
        })
        .collect();

    let mut atr_val = trs.iter().take(period).sum::<f64>() / period as f64;
    for &tr in &trs[period..] {
        atr_val = (atr_val * (period as f64 - 1.0) + tr) / period as f64;
    }
    Some(atr_val)
}

/// Wilder's Relative Strength Index on a slice of closing prices.
///
/// Returns `None` if there are fewer than `period + 1` closes.
/// A flat input (all closes equal) returns `Some(50.0)` by convention.
pub fn rsi(closes: &[f64], period: usize) -> Option<f64> {
    if period == 0 || closes.len() <= period {
        return None;
    }
    let deltas: Vec<f64> = closes.windows(2).map(|w| w[1] - w[0]).collect();

    let mut avg_gain = deltas[..period].iter().map(|&d| d.max(0.0)).sum::<f64>() / period as f64;
    let mut avg_loss = deltas[..period].iter().map(|&d| (-d).max(0.0)).sum::<f64>() / period as f64;

    for &d in &deltas[period..] {
        let gain = d.max(0.0);
        let loss = (-d).max(0.0);
        avg_gain = (avg_gain * (period as f64 - 1.0) + gain) / period as f64;
        avg_loss = (avg_loss * (period as f64 - 1.0) + loss) / period as f64;
    }

    if avg_loss == 0.0 {
        if avg_gain == 0.0 {
            return Some(50.0);
        }
        return Some(100.0);
    }
    let rs = avg_gain / avg_loss;
    Some(100.0 - 100.0 / (1.0 + rs))
}

/// Lowest low across the last `period` bars (inclusive of the most recent).
pub fn swing_low(bars: &[HistoricalBar], period: usize) -> Option<f64> {
    if period == 0 || bars.len() < period {
        return None;
    }
    bars.iter()
        .rev()
        .take(period)
        .map(|b| b.low)
        .fold(None, |acc, x| match acc {
            None => Some(x),
            Some(m) => Some(m.min(x)),
        })
}

/// Highest high across the last `period` bars (inclusive of the most recent).
pub fn swing_high(bars: &[HistoricalBar], period: usize) -> Option<f64> {
    if period == 0 || bars.len() < period {
        return None;
    }
    bars.iter()
        .rev()
        .take(period)
        .map(|b| b.high)
        .fold(None, |acc, x| match acc {
            None => Some(x),
            Some(m) => Some(m.max(x)),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar(open: f64, high: f64, low: f64, close: f64, volume: i64) -> HistoricalBar {
        HistoricalBar {
            time: "20240101".into(),
            open,
            high,
            low,
            close,
            volume,
            wap: close,
            count: 0,
        }
    }

    fn flat_bar(price: f64) -> HistoricalBar {
        bar(price, price, price, price, 1_000)
    }

    /// Wilder's RSI reference fixture. From "New Concepts in Technical Trading
    /// Systems" by J. Welles Wilder (1978). RSI(14) over the 15 closes below
    /// is ≈ 70.46 (rounding of `100 - 100/(1+RS)` where RS = 0.2386/0.10).
    #[test]
    fn rsi_matches_wilder_reference_fixture() {
        let closes = vec![
            44.34, 44.09, 44.15, 43.61, 44.33, 44.83, 45.10, 45.42, 45.84, 46.08, 45.89, 46.03,
            45.61, 46.28, 46.28,
        ];
        let v = rsi(&closes, 14).expect("rsi");
        assert!((v - 70.46).abs() < 0.05, "expected ~70.46, got {v}");
    }

    #[test]
    fn rsi_constant_uptrend_is_100() {
        let closes: Vec<f64> = (0..20).map(|i| 10.0 + i as f64).collect();
        let v = rsi(&closes, 14).expect("rsi");
        assert!((v - 100.0).abs() < 1e-9, "expected 100, got {v}");
    }

    #[test]
    fn rsi_constant_downtrend_is_0() {
        let closes: Vec<f64> = (0..20).map(|i| 50.0 - i as f64).collect();
        let v = rsi(&closes, 14).expect("rsi");
        assert!((v - 0.0).abs() < 1e-9, "expected 0, got {v}");
    }

    #[test]
    fn rsi_flat_input_is_50() {
        let closes = vec![10.0; 20];
        let v = rsi(&closes, 14).expect("rsi");
        assert!((v - 50.0).abs() < 1e-9, "expected 50, got {v}");
    }

    #[test]
    fn rsi_returns_none_when_insufficient_history() {
        let closes = vec![10.0, 11.0, 12.0];
        assert!(rsi(&closes, 14).is_none());
    }

    #[test]
    fn atr_constant_true_range_returns_that_value() {
        // Each bar has H-L = 1.0 with no gaps → TR = 1.0 every bar → ATR = 1.0.
        let bars: Vec<HistoricalBar> = (0..20).map(|_| bar(9.5, 10.0, 9.0, 9.5, 1_000)).collect();
        let v = atr(&bars, 14).expect("atr");
        assert!((v - 1.0).abs() < 1e-9, "expected 1.0, got {v}");
    }

    #[test]
    fn atr_flat_bars_is_zero() {
        let bars = vec![flat_bar(50.0); 20];
        let v = atr(&bars, 14).expect("atr");
        assert!(v.abs() < 1e-9, "expected 0, got {v}");
    }

    #[test]
    fn atr_returns_none_when_insufficient_bars() {
        let bars: Vec<HistoricalBar> = (0..14).map(|_| flat_bar(10.0)).collect();
        // Need period + 1 bars; 14 is one short.
        assert!(atr(&bars, 14).is_none());
    }

    #[test]
    fn swing_low_finds_min_over_window() {
        let bars = vec![
            bar(10.0, 11.0, 9.5, 10.5, 0),
            bar(10.5, 11.5, 9.2, 10.8, 0),
            bar(10.8, 12.0, 10.0, 11.5, 0),
            bar(11.5, 12.5, 8.8, 11.0, 0),
            bar(11.0, 11.5, 10.2, 11.2, 0),
        ];
        // Last 3 bars: lows 10.0, 8.8, 10.2 → 8.8
        assert_eq!(swing_low(&bars, 3), Some(8.8));
        // Last 2 bars: 8.8, 10.2 → 8.8
        assert_eq!(swing_low(&bars, 2), Some(8.8));
        // Single most recent bar: 10.2
        assert_eq!(swing_low(&bars, 1), Some(10.2));
    }

    #[test]
    fn swing_high_finds_max_over_window() {
        let bars = vec![
            bar(10.0, 11.0, 9.5, 10.5, 0),
            bar(10.5, 13.5, 9.2, 10.8, 0),
            bar(10.8, 12.0, 10.0, 11.5, 0),
            bar(11.5, 12.5, 9.8, 11.0, 0),
            bar(11.0, 11.5, 10.2, 11.2, 0),
        ];
        // Last 3 bars: highs 12.0, 12.5, 11.5 → 12.5
        assert_eq!(swing_high(&bars, 3), Some(12.5));
        // Last 5 bars: 13.5 dominates
        assert_eq!(swing_high(&bars, 5), Some(13.5));
    }

    #[test]
    fn swing_helpers_reject_oversize_window() {
        let bars = vec![flat_bar(10.0); 3];
        assert!(swing_low(&bars, 5).is_none());
        assert!(swing_high(&bars, 5).is_none());
        assert!(swing_low(&bars, 0).is_none());
    }
}
