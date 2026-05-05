//! Pure deterministic grade computation. The LLM never picks the grade.
//!
//! `score = clamp(net_pnl / 100, -25, 25) + sum(tag_weights)`
//! Banding: `>=25 A`; `>=10 B`; `>=-5 C`; `>=-20 D`; else `F`.

use serde::{Deserialize, Serialize};

use super::tags::BehavioralTag;
use super::types::LegSummary;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum GradeLetter {
    A,
    B,
    C,
    D,
    F,
}

impl GradeLetter {
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Grade {
    pub grade: GradeLetter,
    pub score: f64,
}

/// `score = clamp(net_pnl / 100, -25, 25) + sum(tag_weights)`.
/// Banding above; thresholds are inclusive.
pub fn compute_grade(summary: &LegSummary, tags: &[BehavioralTag]) -> Grade {
    let pnl_normalised = (summary.net_pnl / 100.0).clamp(-25.0, 25.0);
    let tag_score: i32 = tags.iter().map(|t| t.weight()).sum();
    let score = pnl_normalised + tag_score as f64;
    let grade = if score >= 25.0 {
        GradeLetter::A
    } else if score >= 10.0 {
        GradeLetter::B
    } else if score >= -5.0 {
        GradeLetter::C
    } else if score >= -20.0 {
        GradeLetter::D
    } else {
        GradeLetter::F
    };
    Grade { grade, score }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(net_pnl: f64) -> LegSummary {
        LegSummary {
            gross_pnl: net_pnl + 20.0,
            net_pnl,
            commissions_total: 20.0,
            n_round_trips: 3,
            n_carryover: 0,
            win_rate: Some(2.0 / 3.0),
            by_symbol: Default::default(),
        }
    }

    #[test]
    fn same_inputs_yield_same_grade() {
        let s = summary(380.0);
        let tags = vec![
            BehavioralTag::FlatClose,
            BehavioralTag::DisciplineOnLoser,
            BehavioralTag::ChaseOwnExit,
        ];
        let g1 = compute_grade(&s, &tags);
        let g2 = compute_grade(&s, &tags);
        assert_eq!(g1.grade, g2.grade);
        assert!((g1.score - g2.score).abs() < 1e-9);
    }

    #[test]
    fn pure_winner_with_discipline_grades_at_least_b() {
        let s = LegSummary {
            gross_pnl: 1500.0,
            net_pnl: 1200.0,
            commissions_total: 30.0,
            n_round_trips: 5,
            n_carryover: 0,
            win_rate: Some(0.9),
            by_symbol: Default::default(),
        };
        let tags = vec![
            BehavioralTag::FlatClose,
            BehavioralTag::DisciplineOnLoser,
            BehavioralTag::ThesisMatchExecuted,
        ];
        let g = compute_grade(&s, &tags);
        assert!(
            matches!(g.grade, GradeLetter::A | GradeLetter::B),
            "grade={:?}",
            g.grade
        );
    }

    #[test]
    fn loser_with_chase_grades_no_better_than_d() {
        let s = LegSummary {
            gross_pnl: -200.0,
            net_pnl: -300.0,
            commissions_total: 100.0,
            n_round_trips: 4,
            n_carryover: 1,
            win_rate: Some(0.25),
            by_symbol: Default::default(),
        };
        let tags = vec![
            BehavioralTag::ChaseOwnExit,
            BehavioralTag::LateOtmLottery,
            BehavioralTag::PostLossRevenge,
        ];
        let g = compute_grade(&s, &tags);
        assert!(
            matches!(g.grade, GradeLetter::D | GradeLetter::F),
            "grade={:?}",
            g.grade
        );
    }

    #[test]
    fn flat_zero_day_grades_c() {
        let s = summary(0.0);
        let tags: Vec<BehavioralTag> = Vec::new();
        let g = compute_grade(&s, &tags);
        assert_eq!(g.grade, GradeLetter::C);
        assert!(g.score.abs() < 1e-9);
    }

    #[test]
    fn pnl_clamps_at_plus_25() {
        let s = summary(50_000.0);
        let g = compute_grade(&s, &[]);
        assert_eq!(g.grade, GradeLetter::A);
        assert!((g.score - 25.0).abs() < 1e-9);
    }

    #[test]
    fn pnl_clamps_at_minus_25() {
        let s = summary(-50_000.0);
        let g = compute_grade(&s, &[]);
        assert_eq!(g.grade, GradeLetter::F);
        assert!((g.score - -25.0).abs() < 1e-9);
    }

    #[test]
    fn determinism_stress_1000_runs() {
        let s = summary(380.0);
        let tags = vec![BehavioralTag::FlatClose, BehavioralTag::ChaseOwnExit];
        let baseline = compute_grade(&s, &tags);
        for _ in 0..1000 {
            let g = compute_grade(&s, &tags);
            assert_eq!(g.grade, baseline.grade);
            assert!((g.score - baseline.score).abs() < 1e-12);
        }
    }
}
