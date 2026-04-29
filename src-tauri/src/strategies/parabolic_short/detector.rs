//! Bidirectional? No — short-only "parabolic short" detector.
//!
//! Identifies blow-off-top names ready to fade. Daily-side gates demand
//! ≥ 3 consecutive up days, each ≥ 5%, cumulative ≥ 40%, price ≥ 2× ATR(20)
//! above the 20-day MA, and RSI(14) ≥ 80. Intraday trigger is the close of
//! the first 15-min bar where `close < open`. Stop is the session high so
//! far. Targets are 2R / 3R below trigger.

use async_trait::async_trait;
use chrono::Utc;
use serde_json::json;

use crate::ibkr::types::{BarSize, StrategyTag};
use crate::strategies::indicators::{atr, rsi};
use crate::strategies::{
    targets_for_risk_profile, DetectorError, Direction, MarketContext, SetupCandidate,
    StrategyDetector,
};

/// ATR(20) needs 21 bars; MA(20) needs 20; RSI(14) needs 15. 21 covers all.
const MIN_BARS_FOR_INDICATORS: usize = 21;
const MA_PERIOD: usize = 20;
const ATR_PERIOD: usize = 20;
const RSI_PERIOD: usize = 14;

const MIN_CONSEC_DAYS: usize = 3;
const MIN_PER_DAY_MOVE: f64 = 0.05;
const MIN_CUMULATIVE_MOVE: f64 = 0.40;
const MIN_ATR_DISTANCE: f64 = 2.0;
const MIN_RSI: f64 = 80.0;

/// Recommended fetch window. Internal gate is `MIN_BARS_FOR_INDICATORS`.
const MIN_LOOKBACK_DAYS: u32 = 25;

const NORM_CONSEC_LO: f64 = 3.0;
const NORM_CONSEC_HI: f64 = 6.0;
const NORM_CUMUL_LO: f64 = 0.40;
const NORM_CUMUL_HI: f64 = 0.80;
const NORM_ATR_DIST_LO: f64 = 2.0;
const NORM_ATR_DIST_HI: f64 = 4.0;
const NORM_RSI_LO: f64 = 80.0;
const NORM_RSI_HI: f64 = 95.0;

#[derive(Debug, Default)]
pub struct ParabolicShortDetector;

impl ParabolicShortDetector {
    fn normalize(x: f64, lo: f64, hi: f64) -> f64 {
        if hi <= lo {
            return 0.0;
        }
        ((x - lo) / (hi - lo)).clamp(0.0, 1.0)
    }

    fn conviction(consec: usize, cumul: f64, atr_dist: f64, rsi_v: f64) -> f64 {
        let c = Self::normalize(consec as f64, NORM_CONSEC_LO, NORM_CONSEC_HI);
        let cm = Self::normalize(cumul, NORM_CUMUL_LO, NORM_CUMUL_HI);
        let ad = Self::normalize(atr_dist, NORM_ATR_DIST_LO, NORM_ATR_DIST_HI);
        let r = Self::normalize(rsi_v, NORM_RSI_LO, NORM_RSI_HI);
        (0.3 * c + 0.3 * cm + 0.2 * ad + 0.2 * r).clamp(0.0, 1.0)
    }
}

#[async_trait]
impl StrategyDetector for ParabolicShortDetector {
    fn name(&self) -> &'static str {
        "parabolic_short"
    }
    fn tag(&self) -> StrategyTag {
        StrategyTag::ParabolicShort
    }
    fn timeframe(&self) -> BarSize {
        BarSize::Min15
    }
    fn min_lookback_days(&self) -> u32 {
        MIN_LOOKBACK_DAYS
    }

    async fn evaluate(
        &self,
        ctx: &MarketContext<'_>,
    ) -> Result<Option<SetupCandidate>, DetectorError> {
        let bars = ctx.daily_bars;
        if bars.len() < MIN_BARS_FOR_INDICATORS {
            return Err(DetectorError::InsufficientBars {
                needed: MIN_BARS_FOR_INDICATORS,
                available: bars.len(),
            });
        }
        let intraday = ctx
            .intraday_bars
            .ok_or(DetectorError::IntradayBarsRequired)?;

        let n = bars.len();
        let today = &bars[n - 1];

        // Walk back from the latest bar, counting strict-up days.
        let mut consec_days: usize = 0;
        for i in (1..n).rev() {
            if bars[i].close > bars[i - 1].close {
                consec_days += 1;
            } else {
                break;
            }
        }

        if consec_days < MIN_CONSEC_DAYS {
            return Ok(None);
        }

        // Per-day moves across the streak.
        let streak_start = n - consec_days;
        let prior_close = bars[streak_start - 1].close;
        if prior_close <= 0.0 {
            return Ok(None);
        }
        let mut min_per_day_move = f64::INFINITY;
        for i in streak_start..n {
            let pc = bars[i - 1].close;
            if pc <= 0.0 {
                return Ok(None);
            }
            let mv = (bars[i].close - pc) / pc;
            if mv < min_per_day_move {
                min_per_day_move = mv;
            }
        }
        if min_per_day_move < MIN_PER_DAY_MOVE {
            return Ok(None);
        }

        let cumulative_move = (today.close - prior_close) / prior_close;
        if cumulative_move < MIN_CUMULATIVE_MOVE {
            return Ok(None);
        }

        // Extension above MA(20) measured in ATR(20) units.
        let ma_window = &bars[n - MA_PERIOD..n];
        let ma_20: f64 = ma_window.iter().map(|b| b.close).sum::<f64>() / MA_PERIOD as f64;
        let atr_20 = atr(bars, ATR_PERIOD)
            .ok_or_else(|| DetectorError::Internal("ATR(20) requires at least 21 bars".into()))?;
        if atr_20 == 0.0 {
            return Ok(None);
        }
        let atr_distance = (today.close - ma_20) / atr_20;
        if atr_distance < MIN_ATR_DISTANCE {
            return Ok(None);
        }

        // Overextension on Wilder RSI(14).
        let closes: Vec<f64> = bars.iter().map(|b| b.close).collect();
        let rsi_14 = rsi(&closes, RSI_PERIOD)
            .ok_or_else(|| DetectorError::Internal("RSI(14) requires at least 15 closes".into()))?;
        if rsi_14 < MIN_RSI {
            return Ok(None);
        }

        // First red intraday bar triggers entry.
        let red_bar = match intraday.iter().find(|b| b.close < b.open) {
            Some(b) => b,
            None => return Ok(None),
        };

        let trigger_price = red_bar.close;
        let stop_price = intraday
            .iter()
            .map(|b| b.high)
            .fold(f64::NEG_INFINITY, f64::max);

        if stop_price <= trigger_price {
            return Ok(None);
        }

        let targets = targets_for_risk_profile(Direction::Short, trigger_price, stop_price)
            .map_err(|e| DetectorError::Internal(e.into()))?;

        let raw_signals = json!({
            "consec_days": consec_days,
            "cumulative_move": cumulative_move,
            "atr_distance": atr_distance,
            "rsi_14": rsi_14,
            "min_per_day_move": min_per_day_move,
            "ma_20": ma_20,
            "atr_20": atr_20,
        });

        Ok(Some(SetupCandidate {
            strategy: "parabolic_short",
            tag: StrategyTag::ParabolicShort,
            direction: Direction::Short,
            conviction_signal: Self::conviction(consec_days, cumulative_move, atr_distance, rsi_14),
            trigger_price,
            stop_price,
            targets,
            raw_signals,
            timeframe: BarSize::Min15,
            detected_at: Utc::now(),
        }))
    }
}
