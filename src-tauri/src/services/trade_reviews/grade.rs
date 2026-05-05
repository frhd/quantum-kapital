//! Phase 4 (quant-decisions): R-edge + discipline scoring.
//!
//! Two surfaced numbers, never summed for ranking (committed in master
//! defaults):
//!
//! 1. `score_v2 = Σ(realized_R × conviction_weight)` over closed
//!    trade legs.
//! 2. `discipline_v2 = Σ(tag_weights)`.
//!
//! `conviction_weight` is calibrated via the per-grade realized-target
//! rate from `eval_harness::calibration_stats`:
//! `conviction_weight = realized_target_rate(grade) / target_rate(C)`.
//! Falls back to a deterministic A=1.5 / B=1.0 / C=1.0 baseline when
//! sample size is below the 50-trade calibration floor; the call site
//! that builds the calibration is responsible for emitting a
//! `tracing::warn!` when the fallback applies. The coupling between
//! Phase-1 sizing (which already conviction-multiplies risk) and
//! Phase-4 grading (which now also rewards conviction) is real and
//! Phase 11 tilt-guard exists in part to clamp it — see
//! `compute_score_v2` for the reminder.
//!
//! `GradeLetter` and the legacy v1 `score`/`grade` field are retained
//! for read-back of pre-P4 rows ONLY — never recomputed for new
//! writes. The phase doc forbids retroactive upgrades and the CI grep
//! invariant forbids the `net_pnl/100` term in production code; both
//! land here.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::attribution::LegWithR;
use super::tags::BehavioralTag;

/// Pre-P4 grade letter. Retained ONLY so the store can parse stored
/// `grade` strings on legacy rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "UPPERCASE")]
pub enum GradeLetter {
    A,
    B,
    C,
    D,
    F,
}

impl GradeLetter {
    #[allow(dead_code)] // surfaced through `Grade::as_str` for v1 read-back
    pub fn as_str(self) -> &'static str {
        match self {
            GradeLetter::A => "A",
            GradeLetter::B => "B",
            GradeLetter::C => "C",
            GradeLetter::D => "D",
            GradeLetter::F => "F",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "A" => Some(GradeLetter::A),
            "B" => Some(GradeLetter::B),
            "C" => Some(GradeLetter::C),
            "D" => Some(GradeLetter::D),
            "F" => Some(GradeLetter::F),
            _ => None,
        }
    }
}

/// Pre-P4 grade tuple. Surfaced on legacy rows only; never produced by
/// new writes. Kept on the public API surface so consumers reading
/// pre-P4 rows can re-pack `(grade, grade_score)` deterministically.
#[allow(dead_code)] // legacy v1 helper retained for downstream consumers
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Grade {
    pub grade: GradeLetter,
    pub score: f64,
}

/// Conviction calibration table. Built from
/// `eval_harness::calibration_stats` at write time; unknown grades or
/// under-sampled buckets fall back to a 1.0 multiplier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConvictionCalibration {
    pub a: f64,
    pub b: f64,
    pub c: f64,
}

impl ConvictionCalibration {
    /// Default that's safe to use until calibration N ≥ 50: rewards A
    /// 1.5× and B/C at parity. Documented in master.md "Defaults
    /// committed".
    pub fn fallback() -> Self {
        Self {
            a: 1.5,
            b: 1.0,
            c: 1.0,
        }
    }

    pub fn weight(&self, grade: Option<&str>) -> f64 {
        match grade {
            Some("A") => self.a,
            Some("B") => self.b,
            Some("C") => self.c,
            _ => 1.0,
        }
    }
}

/// V2 score-and-discipline output. The two numbers are kept distinct
/// — never summed (master commitment).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ScoreV2 {
    pub score_v2: f64,
    pub discipline_v2: f64,
    /// Trade-legs that contributed an R term to `score_v2`.
    pub n_legs_with_r: usize,
    /// Trade-legs that did not (no setup link or NULL dollar_risk on
    /// the linked setup). Surfaces "your setup wiring has gaps".
    pub n_legs_unattributed: usize,
    /// `"v2"` — pinned at write time.
    pub formula_version: String,
}

/// Compute `score_v2` from legs and a conviction calibration.
///
/// Coupling reminder: Phase-1 sizing already conviction-multiplies
/// risk (so an A-conviction setup is sized larger). Phase 4 grading
/// rewards realized R, which is `pnl / dollar_risk`, then *also*
/// scales by conviction_weight. If A conviction miscalibrates upward,
/// both sizing AND grade reward error. Phase 11 tilt guard clamps
/// runaway days; until P11 lands, the fallback calibration of 1.5/1.0
/// /1.0 is intentionally conservative.
pub fn compute_score_v2(
    legs: &[LegWithR<'_>],
    calibration: &ConvictionCalibration,
    leg_conviction: impl Fn(&LegWithR<'_>) -> Option<String>,
    tags: &[BehavioralTag],
) -> ScoreV2 {
    let mut score = 0.0;
    let mut n_with = 0_usize;
    let mut n_without = 0_usize;
    for l in legs {
        match l.realized_r {
            Some(r) => {
                let weight = calibration.weight(leg_conviction(l).as_deref());
                score += r * weight;
                n_with += 1;
            }
            None => {
                n_without += 1;
            }
        }
    }
    ScoreV2 {
        score_v2: score,
        discipline_v2: compute_discipline_v2(tags),
        n_legs_with_r: n_with,
        n_legs_unattributed: n_without,
        formula_version: "v2".into(),
    }
}

/// `discipline_v2 = Σ(tag.weight())`. Negative for typical days
/// (tag weights skew negative). Surfaced as a deficit, not a positive.
pub fn compute_discipline_v2(tags: &[BehavioralTag]) -> f64 {
    tags.iter().map(|t| t.weight() as f64).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::trade_legs::{LegTag, TradeLeg};
    use chrono::{TimeZone, Utc};

    fn leg(net_pnl: f64) -> TradeLeg {
        TradeLeg {
            leg_id: "L".into(),
            account: "U1".into(),
            symbol: "X".into(),
            contract_type: "STK".into(),
            expiry: None,
            strike: None,
            right: None,
            multiplier: None,
            opened_at: Utc.with_ymd_and_hms(2026, 5, 4, 14, 0, 0).unwrap(),
            closed_at: None,
            buy_qty: 100.0,
            avg_buy_price: 1.0,
            sell_qty: 100.0,
            avg_sell_price: 1.0,
            gross_pnl: net_pnl,
            commission_total: 0.0,
            net_pnl,
            hold_minutes: None,
            source_exec_ids: vec![],
            tags: vec![LegTag::RoundTrip],
            strategy: None,
            setup_id: None,
        }
    }

    #[test]
    fn discipline_v2_sums_tag_weights() {
        let tags = vec![
            BehavioralTag::FlatClose,         // +5
            BehavioralTag::DisciplineOnLoser, // +5
            BehavioralTag::ChaseOwnExit,      // -10
        ];
        let d = compute_discipline_v2(&tags);
        assert!((d - 0.0).abs() < 1e-9);
    }

    #[test]
    fn score_v2_rewards_realized_r_with_conviction() {
        let l1 = leg(100.0);
        let l2 = leg(-50.0);
        let inputs = vec![
            LegWithR {
                leg: &l1,
                realized_r: Some(2.0),
            },
            LegWithR {
                leg: &l2,
                realized_r: Some(-1.0),
            },
        ];
        let cal = ConvictionCalibration {
            a: 1.5,
            b: 1.0,
            c: 0.5,
        };
        let conv = |w: &LegWithR<'_>| {
            if w.leg.net_pnl > 0.0 {
                Some("A".into())
            } else {
                Some("C".into())
            }
        };
        let s = compute_score_v2(&inputs, &cal, conv, &[]);
        // 2.0 * 1.5 + (-1.0 * 0.5) = 3.0 - 0.5 = 2.5
        assert!((s.score_v2 - 2.5).abs() < 1e-9);
        assert_eq!(s.n_legs_with_r, 2);
        assert_eq!(s.n_legs_unattributed, 0);
        assert_eq!(s.formula_version, "v2");
    }

    #[test]
    fn unattributed_legs_are_counted_but_not_scored() {
        let l = leg(100.0);
        let inputs = vec![LegWithR {
            leg: &l,
            realized_r: None,
        }];
        let cal = ConvictionCalibration::fallback();
        let s = compute_score_v2(&inputs, &cal, |_| None, &[BehavioralTag::FlatClose]);
        assert!((s.score_v2 - 0.0).abs() < 1e-9);
        assert_eq!(s.n_legs_unattributed, 1);
        assert!((s.discipline_v2 - 5.0).abs() < 1e-9);
    }

    #[test]
    fn fallback_calibration_is_conservative() {
        let cal = ConvictionCalibration::fallback();
        assert_eq!(cal.a, 1.5);
        assert_eq!(cal.b, 1.0);
        assert_eq!(cal.c, 1.0);
    }

    #[test]
    fn grade_letter_round_trip_parse() {
        for s in ["A", "B", "C", "D", "F"] {
            let g = GradeLetter::parse(s).unwrap();
            assert_eq!(g.as_str(), s);
        }
        assert!(GradeLetter::parse("Z").is_none());
    }
}
