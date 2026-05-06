//! Phase 9 — pure mapping from `RegimeInputs` to `Regime` axes. No I/O.
//!
//! Thresholds are conservative and match the master plan's "Defaults
//! committed" intent: bucketing is coarse so the classifier doesn't
//! whipsaw on tiny moves around a boundary. The 3-day persistence
//! rule lives one level up, in [`super::types::Regime::apply_persistence`].

use super::inputs::RegimeInputs;
use super::types::{BreadthAxis, CorrAxis, Regime, TrendAxis, VolAxis};

/// VIX bucket boundaries. Picked from the historical VIX distribution:
/// roughly the 25th/75th percentile of the last 10 years.
const VIX_LOW_THRESHOLD: f64 = 14.0;
const VIX_HIGH_THRESHOLD: f64 = 22.0;

/// Breadth thresholds — % of universe trading above its 50-DMA.
const BREADTH_HEALTHY_PCT: f64 = 0.55;
const BREADTH_NARROW_PCT: f64 = 0.35;

/// Correlation buckets per phase doc: Low < 0.5, High >= 0.5.
/// Mid-band (0.4–0.5) collapsed to `Mixed` so a small move across the
/// boundary doesn't flip the gate every other day. Phase doc's
/// 2-bucket scheme (`Low < 0.5`, `High >= 0.5`) widens to 3 here for
/// shape consistency with the other axes; downstream filters that
/// only care about Low / High treat Mixed as "any".
const CORR_LOW_THRESHOLD: f64 = 0.40;
const CORR_HIGH_THRESHOLD: f64 = 0.50;

/// Slope of SPY's 50-DMA over 10 sessions, expressed as a fraction.
/// |slope| > 1% over 10d marks a trending regime; otherwise sideways.
const SPY_TREND_SLOPE_THRESHOLD: f64 = 0.01;

pub fn classify(inputs: &RegimeInputs) -> Regime {
    Regime {
        trend: classify_trend(inputs),
        vol: classify_vol(inputs),
        breadth: classify_breadth(inputs),
        corr: classify_corr(inputs),
    }
}

fn classify_trend(inputs: &RegimeInputs) -> TrendAxis {
    let Some(spy) = &inputs.spy else {
        return TrendAxis::Sideways;
    };
    let above_200 = spy.last_close > spy.ma200;
    let above_50 = spy.last_close > spy.ma50;
    let slope_up = spy.ma50_slope_10d > SPY_TREND_SLOPE_THRESHOLD;
    let slope_down = spy.ma50_slope_10d < -SPY_TREND_SLOPE_THRESHOLD;

    match (above_200, above_50, slope_up, slope_down) {
        // Up: above both MAs and 50-DMA rising.
        (true, true, true, _) => TrendAxis::Up,
        // Down: below both MAs and 50-DMA falling.
        (false, false, _, true) => TrendAxis::Down,
        // Mixed slope or mixed MA position → sideways.
        _ => TrendAxis::Sideways,
    }
}

fn classify_vol(inputs: &RegimeInputs) -> VolAxis {
    let Some(vix) = &inputs.vix else {
        return VolAxis::Normal;
    };
    if vix.last_close < VIX_LOW_THRESHOLD {
        VolAxis::Low
    } else if vix.last_close >= VIX_HIGH_THRESHOLD {
        VolAxis::High
    } else {
        VolAxis::Normal
    }
}

fn classify_breadth(inputs: &RegimeInputs) -> BreadthAxis {
    let Some(b) = &inputs.breadth else {
        return BreadthAxis::Mixed;
    };
    if b.pct_above_50ma >= BREADTH_HEALTHY_PCT {
        BreadthAxis::Healthy
    } else if b.pct_above_50ma <= BREADTH_NARROW_PCT {
        BreadthAxis::Narrow
    } else {
        BreadthAxis::Mixed
    }
}

fn classify_corr(inputs: &RegimeInputs) -> CorrAxis {
    let Some(c) = &inputs.corr else {
        return CorrAxis::Mixed;
    };
    if c.avg_pairwise_corr <= CORR_LOW_THRESHOLD {
        CorrAxis::Low
    } else if c.avg_pairwise_corr >= CORR_HIGH_THRESHOLD {
        CorrAxis::High
    } else {
        CorrAxis::Mixed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::regime::inputs::{
        BreadthInputs, CorrInputs, RegimeInputs, SpyInputs, VixInputs,
    };

    fn empty_inputs() -> RegimeInputs {
        RegimeInputs {
            spy: None,
            vix: None,
            breadth: None,
            corr: None,
            missing: vec![],
        }
    }

    #[test]
    fn missing_inputs_collapse_to_neutral_axes() {
        let r = classify(&empty_inputs());
        assert_eq!(r.trend, TrendAxis::Sideways);
        assert_eq!(r.vol, VolAxis::Normal);
        assert_eq!(r.breadth, BreadthAxis::Mixed);
        assert_eq!(r.corr, CorrAxis::Mixed);
    }

    #[test]
    fn vix_spike_day_classifies_high_vol() {
        let mut inputs = empty_inputs();
        inputs.vix = Some(VixInputs {
            last_close: 28.5,
            change_5d: 0.42,
        });
        assert_eq!(classify(&inputs).vol, VolAxis::High);
    }

    #[test]
    fn quiet_vix_classifies_low_vol() {
        let mut inputs = empty_inputs();
        inputs.vix = Some(VixInputs {
            last_close: 12.5,
            change_5d: -0.05,
        });
        assert_eq!(classify(&inputs).vol, VolAxis::Low);
    }

    #[test]
    fn spy_above_both_mas_with_rising_slope_is_up_trend() {
        let mut inputs = empty_inputs();
        inputs.spy = Some(SpyInputs {
            last_close: 500.0,
            ma50: 480.0,
            ma200: 450.0,
            ma50_slope_10d: 0.025,
        });
        assert_eq!(classify(&inputs).trend, TrendAxis::Up);
    }

    #[test]
    fn spy_below_both_mas_with_falling_slope_is_down_trend() {
        let mut inputs = empty_inputs();
        inputs.spy = Some(SpyInputs {
            last_close: 410.0,
            ma50: 430.0,
            ma200: 460.0,
            ma50_slope_10d: -0.022,
        });
        assert_eq!(classify(&inputs).trend, TrendAxis::Down);
    }

    #[test]
    fn spy_above_200_below_50_is_sideways() {
        let mut inputs = empty_inputs();
        inputs.spy = Some(SpyInputs {
            last_close: 460.0,
            ma50: 470.0,
            ma200: 450.0,
            ma50_slope_10d: 0.0,
        });
        assert_eq!(classify(&inputs).trend, TrendAxis::Sideways);
    }

    #[test]
    fn breadth_buckets_clamp_to_thresholds() {
        let mut inputs = empty_inputs();
        inputs.breadth = Some(BreadthInputs {
            pct_above_50ma: 0.7,
            coverage: 0.95,
        });
        assert_eq!(classify(&inputs).breadth, BreadthAxis::Healthy);

        inputs.breadth = Some(BreadthInputs {
            pct_above_50ma: 0.45,
            coverage: 0.95,
        });
        assert_eq!(classify(&inputs).breadth, BreadthAxis::Mixed);

        inputs.breadth = Some(BreadthInputs {
            pct_above_50ma: 0.20,
            coverage: 0.95,
        });
        assert_eq!(classify(&inputs).breadth, BreadthAxis::Narrow);
    }

    #[test]
    fn correlation_buckets_use_master_thresholds() {
        let mut inputs = empty_inputs();
        inputs.corr = Some(CorrInputs {
            avg_pairwise_corr: 0.30,
            coverage: 0.9,
        });
        assert_eq!(classify(&inputs).corr, CorrAxis::Low);

        inputs.corr = Some(CorrInputs {
            avg_pairwise_corr: 0.45,
            coverage: 0.9,
        });
        assert_eq!(classify(&inputs).corr, CorrAxis::Mixed);

        inputs.corr = Some(CorrInputs {
            avg_pairwise_corr: 0.65,
            coverage: 0.9,
        });
        assert_eq!(classify(&inputs).corr, CorrAxis::High);
    }
}
