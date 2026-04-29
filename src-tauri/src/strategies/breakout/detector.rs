//! Long-only breakout detector.
//!
//! Fires when today's close makes a new N-day-high close on volume ≥
//! `cfg.volume_multiple`× the prior-N-day average, provided RSI(14) is below
//! `cfg.rsi_ceiling` (not overextended). Stop is the tighter of the
//! `cfg.swing_low_period`-bar swing low or `close − 1×ATR(cfg.atr_period)`.
//! Targets are 2R / 3R above trigger. Defaults match the Phase 07 baseline.

use async_trait::async_trait;
use chrono::Utc;
use serde_json::json;

use crate::ibkr::types::{BarSize, StrategyTag};
use crate::strategies::config::BreakoutCfg;
use crate::strategies::indicators::{atr, rsi, swing_low};
use crate::strategies::{
    targets_for_risk_profile, DetectorError, Direction, MarketContext, SetupCandidate,
    StrategyDetector,
};

const RSI_PERIOD: usize = 14;
/// Buffer above `cfg.lookback_days` so the warm-up window has enough bars
/// for swing-low / ATR / RSI seed periods.
const MIN_LOOKBACK_BUFFER: u32 = 10;
/// Logistic steepness for the conviction signal. With midpoint at the
/// configured `volume_multiple`, `k = 1.2` gives ≈0.5 at 1.5× and ≈0.86 at 3.0×.
const CONVICTION_K: f64 = 1.2;

#[derive(Debug, Clone, Default)]
pub struct BreakoutDetector {
    cfg: BreakoutCfg,
}

impl BreakoutDetector {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_config(cfg: BreakoutCfg) -> Self {
        Self { cfg }
    }

    fn min_bars(&self) -> u32 {
        self.cfg.lookback_days + MIN_LOOKBACK_BUFFER
    }

    fn conviction(&self, vol_mult: f64) -> f64 {
        let raw = 1.0 / (1.0 + (-CONVICTION_K * (vol_mult - self.cfg.volume_multiple)).exp());
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
        self.min_bars()
    }

    async fn evaluate(
        &self,
        ctx: &MarketContext<'_>,
    ) -> Result<Option<SetupCandidate>, DetectorError> {
        let bars = ctx.daily_bars;
        let lookback_days = self.cfg.lookback_days as usize;
        let atr_period = self.cfg.atr_period as usize;
        let swing_low_period = self.cfg.swing_low_period as usize;
        let needed = self.min_bars() as usize;
        if bars.len() < needed {
            return Err(DetectorError::InsufficientBars {
                needed,
                available: bars.len(),
            });
        }
        let n = bars.len();
        let today = &bars[n - 1];

        // N-day prior high (exclusive of today).
        let prior_window = &bars[n - 1 - lookback_days..n - 1];
        let lookback_high = prior_window
            .iter()
            .map(|b| b.close)
            .fold(f64::NEG_INFINITY, f64::max);

        // Volume multiple over the same exclusive window.
        let vol_avg: f64 =
            prior_window.iter().map(|b| b.volume as f64).sum::<f64>() / lookback_days as f64;
        let vol_mult = if vol_avg > 0.0 {
            today.volume as f64 / vol_avg
        } else {
            0.0
        };

        // Indicators on the full bar slice (latest bar inclusive).
        let atr_n = atr(bars, atr_period).ok_or_else(|| {
            DetectorError::Internal(format!(
                "ATR({atr_period}) requires at least {} bars",
                atr_period + 1
            ))
        })?;
        let swing_low_n = swing_low(bars, swing_low_period).ok_or_else(|| {
            DetectorError::Internal(format!(
                "swing_low({swing_low_period}) requires at least {swing_low_period} bars"
            ))
        })?;
        let closes: Vec<f64> = bars.iter().map(|b| b.close).collect();
        let rsi_14 = rsi(&closes, RSI_PERIOD)
            .ok_or_else(|| DetectorError::Internal("RSI(14) requires at least 15 closes".into()))?;

        let trigger_price = today.close;
        let raw_signals = json!({
            "lookback_high": lookback_high,
            "volume_multiple": vol_mult,
            "atr_14": atr_n,
            "swing_low_10": swing_low_n,
            "rsi_14": rsi_14,
        });

        // Trigger predicates.
        if trigger_price < lookback_high {
            return Ok(None);
        }
        if vol_mult < self.cfg.volume_multiple {
            return Ok(None);
        }
        if rsi_14 >= self.cfg.rsi_ceiling {
            return Ok(None);
        }

        // Stop: the *higher* (tighter) of the swing-low or trigger - 1×ATR.
        let stop_price = swing_low_n.max(trigger_price - atr_n);
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
            conviction_signal: self.conviction(vol_mult),
            trigger_price,
            stop_price,
            targets,
            raw_signals,
            timeframe: BarSize::Day1,
            detected_at: Utc::now(),
        }))
    }
}
