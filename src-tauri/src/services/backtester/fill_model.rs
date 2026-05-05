//! Phase 6 — `FillModel` trait + two implementations.
//!
//! The fill model converts an *intended* price (entry trigger / stop /
//! target) into an *actual* fill price after slippage. Both sides:
//! entry slippage is "trader paid worse than intended"; exit slippage
//! at a stop/target is "trader filled worse than intended".
//!
//! The trait is `Sync`-bounded but methods are `&mut self` because the
//! calibrated model carries an `RngState`. The replay loop wraps each
//! per-symbol pass in its own `Box<dyn FillModel>` clone so per-symbol
//! sampling is independent (and reproducible from the seed).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::services::tca::SlippageDistributionRow;
use crate::strategies::Direction;

use super::spec::FillModelKind;

/// A signed-bps adjustment. Positive ⇒ trader paid more / received
/// less than the intent; negative is the (rare) favorable slippage
/// case the calibrated model occasionally samples.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlippageBps(pub i32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FillSide {
    /// Opening a new position (long buy / short sell).
    Entry,
    /// Closing an existing position (long sell / short buy).
    Exit,
}

pub trait FillModel: Send {
    /// Apply slippage to `intended_price` for a `(direction, side)`
    /// trade on `strategy`. `intended_price` is what the bracket /
    /// detector wanted; the return is the price the model believes
    /// the trade actually filled at.
    fn fill_price(
        &mut self,
        strategy: &str,
        direction: Direction,
        side: FillSide,
        intended_price: f64,
    ) -> f64;
}

/// Apply a signed bps slippage to `intended_price` under the
/// trader-cost convention. Returns a price guaranteed `> 0` for
/// positive `intended_price` (we floor at 1e-6 to dodge degenerate
/// downstream divisions).
pub fn apply_slippage(intended_price: f64, direction: Direction, side: FillSide, bps: i32) -> f64 {
    if !intended_price.is_finite() || intended_price <= 0.0 {
        return intended_price;
    }
    // Sign convention: trader cost is positive bps.
    // - Long entry: pay above intent ⇒ + bps
    // - Long exit:  receive below intent ⇒ - bps
    // - Short entry: receive below intent ⇒ - bps  (sold for less)
    // - Short exit:  pay above intent ⇒ + bps  (covered for more)
    let cost_sign: f64 = match (direction, side) {
        (Direction::Long, FillSide::Entry) => 1.0,
        (Direction::Long, FillSide::Exit) => -1.0,
        (Direction::Short, FillSide::Entry) => -1.0,
        (Direction::Short, FillSide::Exit) => 1.0,
    };
    let factor = 1.0 + cost_sign * f64::from(bps) / 10_000.0;
    (intended_price * factor).max(1e-6)
}

/// Symmetric naive fill model: every trade gets the same |bps| of
/// slippage. Cheap and reproducible — used as the pre-P2 default.
#[derive(Debug, Clone, Copy)]
pub struct NaiveNextOpenFill {
    pub bps: u32,
}

impl FillModel for NaiveNextOpenFill {
    fn fill_price(
        &mut self,
        _strategy: &str,
        direction: Direction,
        side: FillSide,
        intended_price: f64,
    ) -> f64 {
        apply_slippage(intended_price, direction, side, self.bps as i32)
    }
}

/// Per-strategy distribution sampled from P2 attribution rows. The
/// distribution carries one (mean_bps, stdev_bps) per strategy; a
/// trade samples from `Normal(mean, stdev)` clamped to a sane range.
/// `RngState` is xorshift64 — fully deterministic given the seed.
#[derive(Debug, Clone)]
pub struct CalibratedFillModel {
    /// Per-strategy `(mean_bps, stdev_bps)`. Strategies missing from
    /// this map fall back to `fallback_bps`.
    pub per_strategy: HashMap<String, (f64, f64)>,
    pub fallback_bps: u32,
    rng: RngState,
}

impl CalibratedFillModel {
    pub fn new(per_strategy: HashMap<String, (f64, f64)>, fallback_bps: u32, seed: u64) -> Self {
        Self {
            per_strategy,
            fallback_bps,
            rng: RngState::new(seed),
        }
    }

    /// Build from P2 `SlippageDistributionRow`s. The histogram buckets
    /// are converted to a per-strategy `(mean, stdev)` using bucket-
    /// midpoints weighted by row counts. The TCA path stores
    /// *absolute* bps, so all entries have non-negative mean — that's
    /// fine for our sampling, which is half-normal-equivalent.
    pub fn from_distribution(
        rows: &[SlippageDistributionRow],
        fallback_bps: u32,
        seed: u64,
    ) -> Self {
        let mut per_strategy: HashMap<String, (f64, f64)> = HashMap::new();
        for row in rows {
            let mut total: f64 = 0.0;
            let mut weighted_sum: f64 = 0.0;
            let mut weighted_sq: f64 = 0.0;
            for b in &row.buckets {
                let lo = b.lower_bps as f64;
                let hi = if b.upper_bps == i64::MAX {
                    (b.lower_bps + 50) as f64
                } else {
                    b.upper_bps as f64
                };
                let mid = (lo + hi) / 2.0;
                let n = b.n as f64;
                total += n;
                weighted_sum += mid * n;
                weighted_sq += mid * mid * n;
            }
            if total < 1.0 {
                continue;
            }
            let mean = weighted_sum / total;
            let var = (weighted_sq / total) - mean * mean;
            let stdev = var.max(0.0).sqrt();
            if let Some(strat) = &row.strategy {
                per_strategy.insert(strat.clone(), (mean, stdev));
            }
        }
        Self::new(per_strategy, fallback_bps, seed)
    }

    fn sample_bps(&mut self, strategy: &str) -> i32 {
        let (mean, stdev) = self
            .per_strategy
            .get(strategy)
            .copied()
            .unwrap_or((f64::from(self.fallback_bps), 0.0));
        let z = self.rng.gauss();
        let raw = mean + stdev * z;
        // Clamp to a sane range — slippage > 1000 bps would be a data
        // bug we don't want to amplify in the backtest.
        let clamped = raw.clamp(-200.0, 1_000.0);
        clamped.round() as i32
    }
}

impl FillModel for CalibratedFillModel {
    fn fill_price(
        &mut self,
        strategy: &str,
        direction: Direction,
        side: FillSide,
        intended_price: f64,
    ) -> f64 {
        let bps = self.sample_bps(strategy);
        apply_slippage(intended_price, direction, side, bps)
    }
}

/// Construct a boxed `FillModel` for `kind`. `tca_rows` is `None` when
/// the spec's calibrated branch can't be sourced (e.g., command thread
/// hasn't fetched yet); the constructor falls back to a naive model in
/// that case to keep the run going.
pub fn build_fill_model(
    kind: &FillModelKind,
    tca_rows: Option<&[SlippageDistributionRow]>,
    seed: u64,
) -> Box<dyn FillModel> {
    match kind {
        FillModelKind::NaiveNextOpen { slippage_bps } => {
            Box::new(NaiveNextOpenFill { bps: *slippage_bps })
        }
        FillModelKind::Calibrated { fallback_bps, .. } => match tca_rows {
            Some(rows) => Box::new(CalibratedFillModel::from_distribution(
                rows,
                *fallback_bps,
                seed,
            )),
            None => Box::new(NaiveNextOpenFill { bps: *fallback_bps }),
        },
    }
}

/// Deterministic xorshift64 + Box-Muller for normal samples. We avoid
/// `rand` to keep the dependency surface small and the seed stable
/// across `rand` major versions.
#[derive(Debug, Clone, Copy)]
pub struct RngState {
    state: u64,
}

impl RngState {
    pub fn new(seed: u64) -> Self {
        // Avoid the all-zero degenerate state — xorshift produces zeros
        // forever from it. Mix in a known nonzero constant.
        let s = if seed == 0 {
            0xa5a5_a5a5_a5a5_a5a5
        } else {
            seed
        };
        Self { state: s }
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }

    fn next_f64(&mut self) -> f64 {
        // Map to [0, 1) by taking the top 53 bits.
        ((self.next_u64() >> 11) as f64) / ((1u64 << 53) as f64)
    }

    /// One sample from `Normal(0, 1)` via Box-Muller. We discard the
    /// second draw — small efficiency hit, but the alternative
    /// (caching draws) complicates determinism across resumes.
    pub fn gauss(&mut self) -> f64 {
        // `f64::EPSILON` floor avoids ln(0) → -inf when next_f64 lands
        // exactly on 0.
        let u1 = self.next_f64().max(f64::EPSILON);
        let u2 = self.next_f64();
        (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::tca::{SlippageBucket, SlippageDistributionRow};

    #[test]
    fn naive_long_entry_costs_more() {
        let mut m = NaiveNextOpenFill { bps: 10 };
        let p = m.fill_price("breakout", Direction::Long, FillSide::Entry, 100.0);
        // 10 bps = 0.10%; long entry pays above intent.
        assert!((p - 100.10).abs() < 1e-9);
    }

    #[test]
    fn naive_long_exit_receives_less() {
        let mut m = NaiveNextOpenFill { bps: 10 };
        let p = m.fill_price("breakout", Direction::Long, FillSide::Exit, 100.0);
        assert!((p - 99.90).abs() < 1e-9);
    }

    #[test]
    fn naive_short_entry_receives_less() {
        let mut m = NaiveNextOpenFill { bps: 10 };
        let p = m.fill_price("breakout", Direction::Short, FillSide::Entry, 100.0);
        assert!((p - 99.90).abs() < 1e-9);
    }

    #[test]
    fn naive_short_exit_pays_more() {
        let mut m = NaiveNextOpenFill { bps: 10 };
        let p = m.fill_price("breakout", Direction::Short, FillSide::Exit, 100.0);
        assert!((p - 100.10).abs() < 1e-9);
    }

    #[test]
    fn apply_slippage_floors_at_positive_epsilon() {
        // Negative intent should pass through unchanged (caller's
        // problem); zero / NaN should be rejected.
        assert!(apply_slippage(0.0, Direction::Long, FillSide::Entry, 10).abs() < 1e-9);
        assert!(apply_slippage(f64::NAN, Direction::Long, FillSide::Entry, 10).is_nan());
    }

    #[test]
    fn rng_is_deterministic_from_seed() {
        let mut a = RngState::new(42);
        let mut b = RngState::new(42);
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn rng_gauss_is_finite() {
        let mut r = RngState::new(7);
        for _ in 0..100 {
            let g = r.gauss();
            assert!(g.is_finite(), "gauss produced non-finite: {g}");
            assert!(g.abs() < 8.0, "gauss outlier: {g}");
        }
    }

    #[test]
    fn calibrated_falls_back_when_strategy_missing() {
        let mut m = CalibratedFillModel::new(HashMap::new(), 5, 42);
        let bps = m.sample_bps("breakout");
        // No distribution → mean=5, stdev=0 → exact fallback.
        assert_eq!(bps, 5);
    }

    #[test]
    fn calibrated_from_distribution_extracts_means() {
        let rows = vec![SlippageDistributionRow {
            strategy: Some("breakout".to_string()),
            liquidity_bucket: "all".to_string(),
            buckets: vec![
                SlippageBucket {
                    lower_bps: 0,
                    upper_bps: 10,
                    n: 10,
                },
                SlippageBucket {
                    lower_bps: 10,
                    upper_bps: 20,
                    n: 10,
                },
            ],
        }];
        let m = CalibratedFillModel::from_distribution(&rows, 0, 42);
        let (mean, stdev) = m.per_strategy.get("breakout").copied().unwrap();
        // Bucket midpoints 5 + 15, equal weights ⇒ mean 10, stdev 5.
        assert!((mean - 10.0).abs() < 1e-9);
        assert!((stdev - 5.0).abs() < 1e-9);
    }

    #[test]
    fn calibrated_is_reproducible_under_same_seed() {
        let rows = vec![SlippageDistributionRow {
            strategy: Some("breakout".to_string()),
            liquidity_bucket: "all".to_string(),
            buckets: vec![SlippageBucket {
                lower_bps: 0,
                upper_bps: 20,
                n: 10,
            }],
        }];
        let mut a = CalibratedFillModel::from_distribution(&rows, 0, 42);
        let mut b = CalibratedFillModel::from_distribution(&rows, 0, 42);
        for _ in 0..50 {
            let pa = a.fill_price("breakout", Direction::Long, FillSide::Entry, 100.0);
            let pb = b.fill_price("breakout", Direction::Long, FillSide::Entry, 100.0);
            assert!((pa - pb).abs() < 1e-12);
        }
    }
}
