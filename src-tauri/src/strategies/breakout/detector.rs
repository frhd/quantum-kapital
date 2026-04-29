//! Long-only breakout detector.
//!
//! Fires when today's close makes a new 20-day-high close on volume ≥ 1.5×
//! the prior-20-day average, provided RSI(14) is below 80 (not overextended).
//! Stop is the tighter of the 10-bar swing low or `close − 1×ATR(14)`.
//! Targets are 2R / 3R above trigger.

use async_trait::async_trait;
use chrono::Utc;
use serde_json::json;

use crate::ibkr::types::{BarSize, StrategyTag};
use crate::strategies::indicators::{atr, rsi, swing_low};
use crate::strategies::{
    targets_for_risk_profile, DetectorError, Direction, MarketContext, SetupCandidate,
    StrategyDetector,
};

const LOOKBACK_DAYS: usize = 20;
const SWING_LOW_PERIOD: usize = 10;
const ATR_PERIOD: usize = 14;
const RSI_PERIOD: usize = 14;
const MIN_VOL_MULTIPLE: f64 = 1.5;
const MAX_RSI: f64 = 80.0;
const MIN_LOOKBACK_DAYS: u32 = 30;
/// Logistic steepness for the conviction signal. With midpoint at the
/// `MIN_VOL_MULTIPLE` (1.5×), `k = 1.2` gives ≈0.5 at 1.5× and ≈0.86 at 3.0×.
const CONVICTION_K: f64 = 1.2;

#[derive(Debug, Default)]
pub struct BreakoutDetector;

impl BreakoutDetector {
    fn conviction(vol_mult: f64) -> f64 {
        let raw = 1.0 / (1.0 + (-CONVICTION_K * (vol_mult - MIN_VOL_MULTIPLE)).exp());
        raw.clamp(0.0, 1.0)
    }
}

#[async_trait]
impl StrategyDetector for BreakoutDetector {
    fn name(&self) -> &'static str {
        "breakout"
    }
    fn tag(&self) -> StrategyTag {
        StrategyTag::Breakout
    }
    fn timeframe(&self) -> BarSize {
        BarSize::Day1
    }
    fn min_lookback_days(&self) -> u32 {
        MIN_LOOKBACK_DAYS
    }

    async fn evaluate(
        &self,
        ctx: &MarketContext<'_>,
    ) -> Result<Option<SetupCandidate>, DetectorError> {
        let bars = ctx.daily_bars;
        let needed = MIN_LOOKBACK_DAYS as usize;
        if bars.len() < needed {
            return Err(DetectorError::InsufficientBars {
                needed,
                available: bars.len(),
            });
        }
        let n = bars.len();
        let today = &bars[n - 1];

        // 20-day prior high (exclusive of today).
        let prior_window = &bars[n - 1 - LOOKBACK_DAYS..n - 1];
        let lookback_high = prior_window
            .iter()
            .map(|b| b.close)
            .fold(f64::NEG_INFINITY, f64::max);

        // Volume multiple over the same exclusive window.
        let vol_avg: f64 =
            prior_window.iter().map(|b| b.volume as f64).sum::<f64>() / LOOKBACK_DAYS as f64;
        let vol_mult = if vol_avg > 0.0 {
            today.volume as f64 / vol_avg
        } else {
            0.0
        };

        // Indicators on the full bar slice (latest bar inclusive).
        let atr_14 = atr(bars, ATR_PERIOD)
            .ok_or_else(|| DetectorError::Internal("ATR(14) requires at least 15 bars".into()))?;
        let swing_low_10 = swing_low(bars, SWING_LOW_PERIOD).ok_or_else(|| {
            DetectorError::Internal("swing_low(10) requires at least 10 bars".into())
        })?;
        let closes: Vec<f64> = bars.iter().map(|b| b.close).collect();
        let rsi_14 = rsi(&closes, RSI_PERIOD)
            .ok_or_else(|| DetectorError::Internal("RSI(14) requires at least 15 closes".into()))?;

        let trigger_price = today.close;
        let raw_signals = json!({
            "lookback_high": lookback_high,
            "volume_multiple": vol_mult,
            "atr_14": atr_14,
            "swing_low_10": swing_low_10,
            "rsi_14": rsi_14,
        });

        // Trigger predicates.
        if trigger_price < lookback_high {
            return Ok(None);
        }
        if vol_mult < MIN_VOL_MULTIPLE {
            return Ok(None);
        }
        if rsi_14 >= MAX_RSI {
            return Ok(None);
        }

        // Stop: the *higher* (tighter) of the swing-low or trigger - 1×ATR.
        let stop_price = swing_low_10.max(trigger_price - atr_14);
        if trigger_price <= stop_price {
            // Degenerate: no risk distance. Skip rather than divide by zero.
            return Ok(None);
        }

        let targets = targets_for_risk_profile(Direction::Long, trigger_price, stop_price)
            .map_err(|e| DetectorError::Internal(e.into()))?;

        Ok(Some(SetupCandidate {
            strategy: "breakout",
            tag: StrategyTag::Breakout,
            direction: Direction::Long,
            conviction_signal: Self::conviction(vol_mult),
            trigger_price,
            stop_price,
            targets,
            raw_signals,
            timeframe: BarSize::Day1,
            detected_at: Utc::now(),
        }))
    }
}
