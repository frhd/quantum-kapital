//! Phase 9 — `Regime` axes + `RegimeFilter`.
//!
//! Four axes × three levels = 81 possible regimes; in practice ~10 are
//! common. The phase doc commits to this coarseness intentionally —
//! fine-grain regimes overfit. Adding a fifth axis requires a written
//! P6-backtest justification, not a casual code change.

use serde::{Deserialize, Serialize};

/// SPY trend axis. Computed from SPY's relationship to its 50-day and
/// 200-day moving averages plus the 50-DMA's slope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrendAxis {
    Up,
    Sideways,
    Down,
}

/// Volatility axis. Computed from VIX level (or fallback) bucketed
/// against the master-committed thresholds. Falls back to `Normal` if
/// the VIX series is missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VolAxis {
    Low,
    Normal,
    High,
}

/// Breadth axis. Computed from "% of the SP500 universe trading above
/// its 50-day MA". Defaults to `Mixed` when the breadth proxy can't
/// be computed (< 80% fresh-bar coverage of the universe).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BreadthAxis {
    Healthy,
    Mixed,
    Narrow,
}

/// Cross-sectional correlation axis. 20-day rolling avg pairwise
/// correlation across the SP500 universe; bucketed Low under 0.5 and
/// High at-or-above 0.5. Mid-band collapsed to `Mixed` so the bucket
/// count stays at three, consistent with the other axes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CorrAxis {
    Low,
    Mixed,
    High,
}

impl TrendAxis {
    pub fn as_str(self) -> &'static str {
        match self {
            TrendAxis::Up => "up",
            TrendAxis::Sideways => "sideways",
            TrendAxis::Down => "down",
        }
    }
}

impl VolAxis {
    pub fn as_str(self) -> &'static str {
        match self {
            VolAxis::Low => "low",
            VolAxis::Normal => "normal",
            VolAxis::High => "high",
        }
    }
}

impl BreadthAxis {
    pub fn as_str(self) -> &'static str {
        match self {
            BreadthAxis::Healthy => "healthy",
            BreadthAxis::Mixed => "mixed",
            BreadthAxis::Narrow => "narrow",
        }
    }
}

impl CorrAxis {
    pub fn as_str(self) -> &'static str {
        match self {
            CorrAxis::Low => "low",
            CorrAxis::Mixed => "mixed",
            CorrAxis::High => "high",
        }
    }
}

/// One classification of the four-axis regime. Persisted to
/// `regime_snapshots.regime_json` as the raw read; the gate-time
/// "stable" view applies the 3-day persistence rule on top.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Regime {
    pub trend: TrendAxis,
    pub vol: VolAxis,
    pub breadth: BreadthAxis,
    pub corr: CorrAxis,
}

impl Regime {
    /// A "neutral" classification used when the inputs are too sparse
    /// to read with confidence. Treated by detector filters as a
    /// no-op match (every detector permits Mixed/Normal).
    pub fn neutral() -> Self {
        Self {
            trend: TrendAxis::Sideways,
            vol: VolAxis::Normal,
            breadth: BreadthAxis::Mixed,
            corr: CorrAxis::Mixed,
        }
    }

    /// Apply the 3-day persistence rule: per-axis, flip from
    /// `prior_stable` to `raw` only if the most recent two raw values
    /// also disagree with `prior_stable` in the same way (i.e. all 3
    /// of `raw` + `prior_raws[0]` + `prior_raws[1]` agree on the new
    /// value). Otherwise carry `prior_stable.axis` forward.
    ///
    /// `prior_raws` is newest-first, NOT including the current `raw`.
    /// When `prior_raws` has fewer than 2 entries (boot-up case), the
    /// rule degrades to "no flip" and `prior_stable` wins.
    pub fn apply_persistence(prior_stable: Regime, raw: Regime, prior_raws: &[Regime]) -> Regime {
        if prior_raws.len() < 2 {
            return prior_stable;
        }
        let r0 = prior_raws[0];
        let r1 = prior_raws[1];

        let trend =
            if raw.trend != prior_stable.trend && r0.trend == raw.trend && r1.trend == raw.trend {
                raw.trend
            } else {
                prior_stable.trend
            };
        let vol = if raw.vol != prior_stable.vol && r0.vol == raw.vol && r1.vol == raw.vol {
            raw.vol
        } else {
            prior_stable.vol
        };
        let breadth = if raw.breadth != prior_stable.breadth
            && r0.breadth == raw.breadth
            && r1.breadth == raw.breadth
        {
            raw.breadth
        } else {
            prior_stable.breadth
        };
        let corr = if raw.corr != prior_stable.corr && r0.corr == raw.corr && r1.corr == raw.corr {
            raw.corr
        } else {
            prior_stable.corr
        };

        Regime {
            trend,
            vol,
            breadth,
            corr,
        }
    }
}

/// Per-detector preferred regimes. A detector fires only when the
/// current `Regime` is a member of every populated axis-set. An empty
/// axis-set means "any value on this axis is fine".
///
/// Defaults are "permit any", so a detector that doesn't override
/// `preferred_regimes()` keeps pre-P9 behavior (regime-agnostic).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RegimeFilter {
    pub trend: Vec<TrendAxis>,
    pub vol: Vec<VolAxis>,
    pub breadth: Vec<BreadthAxis>,
    pub corr: Vec<CorrAxis>,
}

impl RegimeFilter {
    /// `true` when every axis the filter constrains is satisfied by
    /// `regime`. Empty constraint vectors auto-pass.
    pub fn matches(&self, regime: &Regime) -> bool {
        let trend_ok = self.trend.is_empty() || self.trend.contains(&regime.trend);
        let vol_ok = self.vol.is_empty() || self.vol.contains(&regime.vol);
        let breadth_ok = self.breadth.is_empty() || self.breadth.contains(&regime.breadth);
        let corr_ok = self.corr.is_empty() || self.corr.contains(&regime.corr);
        trend_ok && vol_ok && breadth_ok && corr_ok
    }

    /// Human-readable summary like "trend in [up, sideways] && vol in
    /// [low, normal]". Used by the SkippedSetupsPanel annotation so the
    /// trader sees *why* the gate refused without re-deriving the
    /// detector's declared preferences.
    pub fn describe(&self) -> String {
        let mut parts = Vec::new();
        if !self.trend.is_empty() {
            parts.push(format!(
                "trend in [{}]",
                self.trend
                    .iter()
                    .map(|t| t.as_str())
                    .collect::<Vec<_>>()
                    .join(",")
            ));
        }
        if !self.vol.is_empty() {
            parts.push(format!(
                "vol in [{}]",
                self.vol
                    .iter()
                    .map(|v| v.as_str())
                    .collect::<Vec<_>>()
                    .join(",")
            ));
        }
        if !self.breadth.is_empty() {
            parts.push(format!(
                "breadth in [{}]",
                self.breadth
                    .iter()
                    .map(|b| b.as_str())
                    .collect::<Vec<_>>()
                    .join(",")
            ));
        }
        if !self.corr.is_empty() {
            parts.push(format!(
                "corr in [{}]",
                self.corr
                    .iter()
                    .map(|c| c.as_str())
                    .collect::<Vec<_>>()
                    .join(",")
            ));
        }
        if parts.is_empty() {
            "any regime".to_string()
        } else {
            parts.join(" && ")
        }
    }
}

/// Source tag for a `regime_snapshots` row. Lets the timeline
/// distinguish the canonical end-of-day classification from the
/// noisier intraday refresh, and from a manual force-recompute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotSource {
    DailyClose,
    Intraday,
    ForceRecompute,
}

impl SnapshotSource {
    pub fn as_str(self) -> &'static str {
        match self {
            SnapshotSource::DailyClose => "daily_close",
            SnapshotSource::Intraday => "intraday",
            SnapshotSource::ForceRecompute => "force_recompute",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(t: TrendAxis, v: VolAxis, b: BreadthAxis, c: CorrAxis) -> Regime {
        Regime {
            trend: t,
            vol: v,
            breadth: b,
            corr: c,
        }
    }

    #[test]
    fn empty_filter_matches_anything() {
        let filter = RegimeFilter::default();
        assert!(filter.matches(&Regime::neutral()));
        assert!(filter.matches(&r(
            TrendAxis::Down,
            VolAxis::High,
            BreadthAxis::Narrow,
            CorrAxis::High
        )));
    }

    #[test]
    fn filter_requires_all_axes_to_match() {
        let filter = RegimeFilter {
            trend: vec![TrendAxis::Up, TrendAxis::Sideways],
            vol: vec![VolAxis::Low, VolAxis::Normal],
            breadth: vec![],
            corr: vec![],
        };
        assert!(filter.matches(&r(
            TrendAxis::Up,
            VolAxis::Normal,
            BreadthAxis::Healthy,
            CorrAxis::Low
        )));
        assert!(!filter.matches(&r(
            TrendAxis::Down,
            VolAxis::Normal,
            BreadthAxis::Healthy,
            CorrAxis::Low
        )));
        assert!(!filter.matches(&r(
            TrendAxis::Up,
            VolAxis::High,
            BreadthAxis::Healthy,
            CorrAxis::Low
        )));
    }

    #[test]
    fn persistence_rule_holds_until_three_in_a_row_agree() {
        let stable = r(
            TrendAxis::Up,
            VolAxis::Normal,
            BreadthAxis::Healthy,
            CorrAxis::Low,
        );
        let new = r(
            TrendAxis::Down,
            VolAxis::Normal,
            BreadthAxis::Healthy,
            CorrAxis::Low,
        );

        // Today is the first day showing Down — no flip.
        let day1 = Regime::apply_persistence(stable, new, &[]);
        assert_eq!(day1.trend, TrendAxis::Up, "single-day flip is suppressed");

        // Two consecutive prior raws disagree (still Up). No flip.
        let day2 = Regime::apply_persistence(stable, new, &[stable, stable]);
        assert_eq!(day2.trend, TrendAxis::Up);

        // Two prior raws agree with today (all 3 say Down). Flip.
        let day3 = Regime::apply_persistence(stable, new, &[new, new]);
        assert_eq!(
            day3.trend,
            TrendAxis::Down,
            "3 consecutive same-axis reads flip the stable view"
        );

        // Mixed prior — only one prior agrees. No flip.
        let day4 = Regime::apply_persistence(stable, new, &[new, stable]);
        assert_eq!(day4.trend, TrendAxis::Up);
    }

    #[test]
    fn persistence_rule_per_axis_independent() {
        let stable = r(
            TrendAxis::Up,
            VolAxis::Normal,
            BreadthAxis::Healthy,
            CorrAxis::Low,
        );
        // Trend flips (3 in a row), vol stays neutral.
        let raw_today = r(
            TrendAxis::Down,
            VolAxis::Normal,
            BreadthAxis::Healthy,
            CorrAxis::Low,
        );
        let prior_raw1 = r(
            TrendAxis::Down,
            VolAxis::High,
            BreadthAxis::Healthy,
            CorrAxis::Low,
        );
        let prior_raw2 = r(
            TrendAxis::Down,
            VolAxis::High,
            BreadthAxis::Healthy,
            CorrAxis::Low,
        );

        let stable_after = Regime::apply_persistence(stable, raw_today, &[prior_raw1, prior_raw2]);
        // Trend: 3 days of Down → flips.
        assert_eq!(stable_after.trend, TrendAxis::Down);
        // Vol: today's raw is Normal (matches stable), so the High
        // priors don't trip the rule. Stable stays Normal.
        assert_eq!(stable_after.vol, VolAxis::Normal);
    }

    #[test]
    fn describe_renders_constrained_axes_only() {
        let filter = RegimeFilter {
            trend: vec![TrendAxis::Up, TrendAxis::Sideways],
            vol: vec![VolAxis::Low],
            breadth: vec![],
            corr: vec![],
        };
        assert_eq!(filter.describe(), "trend in [up,sideways] && vol in [low]");
    }

    #[test]
    fn describe_handles_unconstrained_filter() {
        assert_eq!(RegimeFilter::default().describe(), "any regime");
    }
}
