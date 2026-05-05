//! Phase 7 — ATR-scaled exit policy.
//!
//! Master decisions committed:
//! - 1× / 2× / 4× ATR(20) at signal time, 50% / 30% / 20% allocation.
//! - Activate chandelier trail after the first target fills (i.e.
//!   ≥ 1×ATR profit). Pre-trigger, stop is fixed at signal stop.
//! - BE move at 1R profit (separate from trail; the trail's stop
//!   already locks in ≥ 1×ATR profit, but on a long where 1R < 1×ATR
//!   the BE move is still load-bearing).
//! - Time-stop horizon per detector (master): breakout=10, episodic=5,
//!   parabolic short=3 trading days.

use super::{
    signed_multiplier, trailing::TrailKind, validate_geometry, ExitPlan, ExitPolicy,
    ExitPolicyContext, ExitPolicyError, ExitTargetSpec, Result, TimeStopSpec, TrailSpec,
    V2_ATR_SCALED,
};

/// Default ATR multiples for the 3-rung ladder. Master decision —
/// configurable per-detector via [`AtrScaled::with_multiples`] if a
/// per-strategy backtest overrides them.
pub const DEFAULT_ATR_MULTIPLES: [f64; 3] = [1.0, 2.0, 4.0];

/// Default qty allocation. Mirrors P3's 50/30/20 because the operator-
/// facing intuition ("half off at first target") is independent of
/// the price model.
pub const DEFAULT_QTY_PCTS: [u8; 3] = [50, 30, 20];

/// Default chandelier ATR multiple — `max(stop_so_far,
/// max_high_since_entry - 3×ATR)` for longs.
pub const DEFAULT_CHANDELIER_ATR_MULTIPLE: f64 = 3.0;

#[derive(Debug, Clone)]
pub struct AtrScaled {
    multiples: [f64; 3],
    qty_pcts: [u8; 3],
    chandelier_atr_multiple: f64,
    time_stop_days: u32,
}

impl AtrScaled {
    /// Master-default builder. `time_stop_days` is the per-detector
    /// horizon (10 / 5 / 3 in master).
    pub fn new(time_stop_days: u32) -> Self {
        Self {
            multiples: DEFAULT_ATR_MULTIPLES,
            qty_pcts: DEFAULT_QTY_PCTS,
            chandelier_atr_multiple: DEFAULT_CHANDELIER_ATR_MULTIPLE,
            time_stop_days,
        }
    }

    /// Overrides for per-strategy calibration when the backtest
    /// suggests a tighter or looser ladder.
    pub fn with_multiples(mut self, multiples: [f64; 3], qty_pcts: [u8; 3]) -> Self {
        self.multiples = multiples;
        self.qty_pcts = qty_pcts;
        self
    }

    pub fn with_chandelier_multiple(mut self, mult: f64) -> Self {
        self.chandelier_atr_multiple = mult;
        self
    }
}

impl ExitPolicy for AtrScaled {
    fn version(&self) -> &'static str {
        V2_ATR_SCALED
    }

    fn build_plan(&self, ctx: &ExitPolicyContext<'_>) -> Result<ExitPlan> {
        let r = validate_geometry(ctx.trigger_price, ctx.stop_price)?;
        let atr = ctx
            .atr
            .ok_or(ExitPolicyError::AtrUnavailable("v2_atr_scaled"))?;
        if !atr.is_finite() || atr <= 0.0 {
            return Err(ExitPolicyError::AtrUnavailable("v2_atr_scaled"));
        }

        let signed = signed_multiplier(ctx.direction);

        // Sum of pcts must be 100; debug_assert in dev so a future
        // override-builder catches the slip locally.
        debug_assert_eq!(self.qty_pcts.iter().map(|p| *p as u32).sum::<u32>(), 100);

        let mut targets = Vec::with_capacity(self.multiples.len());
        for (idx, (&mult, &pct)) in self
            .multiples
            .iter()
            .zip(self.qty_pcts.iter())
            .enumerate()
        {
            // R-multiple metadata is derived for the UI: at signal
            // time r and atr are both fixed scalars, so converting
            // is a static ratio.
            let r_mult = (mult * atr) / r;
            let label = label_for_rung(idx, mult);
            targets.push(ExitTargetSpec {
                label,
                price: ctx.trigger_price + signed * mult * atr,
                qty_pct: pct,
                r_multiple: Some(r_mult),
                atr_multiple: Some(mult),
            });
        }

        let trail = TrailSpec {
            kind: TrailKind::Chandelier,
            atr_multiple: self.chandelier_atr_multiple,
            // Activate trail once the first rung fills — that is, the
            // bracket has booked ≥ 1×ATR of profit. Pre-fill the stop
            // is the original stop_price.
            activate_after_label: Some(label_for_rung(0, self.multiples[0])),
            // BE move at 1R profit, independent of ATR-scaled trail
            // (master committed both rules — they cooperate, not
            // compete).
            move_to_break_even_at_r: Some(1.0),
        };

        let time_stop = TimeStopSpec {
            max_trading_days: self.time_stop_days,
        };

        Ok(ExitPlan {
            policy_version: V2_ATR_SCALED.to_string(),
            targets,
            trail: Some(trail),
            time_stop: Some(time_stop),
            atr_at_signal: Some(atr),
        })
    }
}

fn label_for_rung(idx: usize, atr_multiple: f64) -> String {
    // Idx 0 = first take-profit, last = "runner". Master phase doc:
    // "1×ATR target / 2×ATR target / 4×ATR runner".
    if idx == 2 {
        format!("{:.0}xATR runner", atr_multiple)
    } else {
        format!("{:.0}xATR", atr_multiple)
    }
}
