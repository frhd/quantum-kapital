//! Tunable thresholds for the production detectors.
//!
//! Defaults mirror the constants encoded in Phases 07–09. Settings from
//! `~/.config/quantum-kapital/settings.json` override these without
//! recompiling; missing sections fall back to defaults so older settings
//! files keep working.

use serde::{Deserialize, Serialize};

use crate::services::event_calendar::{BlackoutPolicy, EarningsPolicy, FomcPolicy};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DetectorsConfig {
    #[serde(default)]
    pub breakout: BreakoutCfg,
    #[serde(default)]
    pub episodic_pivot: EpisodicPivotCfg,
    #[serde(default)]
    pub parabolic_short: ParabolicShortCfg,
}

impl DetectorsConfig {
    /// Phase 5 — pick the blackout policy for `strategy`. Match falls
    /// through to a permissive default for unknown strategy strings so
    /// a future detector wired before its config landing doesn't trip
    /// the gate accidentally.
    pub fn blackout_policy_for(&self, strategy: &str) -> BlackoutPolicy {
        match strategy {
            "breakout" => self.breakout.blackout(),
            "episodic_pivot" => self.episodic_pivot.blackout(),
            "parabolic_short" => self.parabolic_short.blackout(),
            _ => BlackoutPolicy::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BreakoutCfg {
    #[serde(default = "default_breakout_lookback_days")]
    pub lookback_days: u32,
    #[serde(default = "default_breakout_volume_multiple")]
    pub volume_multiple: f64,
    #[serde(default = "default_breakout_rsi_ceiling")]
    pub rsi_ceiling: f64,
    #[serde(default = "default_breakout_atr_period")]
    pub atr_period: u32,
    #[serde(default = "default_breakout_swing_low_period")]
    pub swing_low_period: u32,
    /// Phase 5 — earnings blackout window. Master plan default for
    /// breakout: full 5 BD pre + 1 BD post. Operators can shrink (or
    /// disable) per-detector via settings.json.
    #[serde(default = "default_breakout_earnings_bd_pre")]
    pub earnings_bd_pre: u32,
    #[serde(default = "default_breakout_earnings_bd_post")]
    pub earnings_bd_post: u32,
    #[serde(default = "default_breakout_skip_if_unknown_earnings")]
    pub skip_if_unknown_earnings: bool,
    #[serde(default = "default_fomc_enabled")]
    pub fomc_blackout_enabled: bool,
}

impl Default for BreakoutCfg {
    fn default() -> Self {
        Self {
            lookback_days: default_breakout_lookback_days(),
            volume_multiple: default_breakout_volume_multiple(),
            rsi_ceiling: default_breakout_rsi_ceiling(),
            atr_period: default_breakout_atr_period(),
            swing_low_period: default_breakout_swing_low_period(),
            earnings_bd_pre: default_breakout_earnings_bd_pre(),
            earnings_bd_post: default_breakout_earnings_bd_post(),
            skip_if_unknown_earnings: default_breakout_skip_if_unknown_earnings(),
            fomc_blackout_enabled: default_fomc_enabled(),
        }
    }
}

impl BreakoutCfg {
    pub fn blackout(&self) -> BlackoutPolicy {
        BlackoutPolicy {
            earnings: EarningsPolicy {
                bd_pre: self.earnings_bd_pre,
                bd_post: self.earnings_bd_post,
                skip_if_unknown: self.skip_if_unknown_earnings,
            },
            fomc: FomcPolicy {
                enabled: self.fomc_blackout_enabled,
            },
        }
    }
}

fn default_breakout_lookback_days() -> u32 {
    20
}
fn default_breakout_volume_multiple() -> f64 {
    1.5
}
fn default_breakout_rsi_ceiling() -> f64 {
    80.0
}
fn default_breakout_atr_period() -> u32 {
    14
}
fn default_breakout_swing_low_period() -> u32 {
    10
}
fn default_breakout_earnings_bd_pre() -> u32 {
    5
}
fn default_breakout_earnings_bd_post() -> u32 {
    1
}
fn default_breakout_skip_if_unknown_earnings() -> bool {
    true
}
fn default_fomc_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpisodicPivotCfg {
    #[serde(default = "default_ep_min_gap_pct")]
    pub min_gap_pct: f64,
    #[serde(default = "default_ep_min_sentiment_abs")]
    pub min_sentiment_abs: f64,
    #[serde(default = "default_ep_min_volume_ratio")]
    pub min_volume_ratio: f64,
    /// Phase 5 — episodic-pivot is *meant* to trade gap-on-news, which
    /// includes earnings news. Master plan default: 0/0 (disabled),
    /// `skip_if_unknown=false`. P6 backtest will report
    /// earnings-bar performance separately so we can tell whether
    /// earnings is the source of edge or pain.
    #[serde(default = "default_ep_earnings_bd_pre")]
    pub earnings_bd_pre: u32,
    #[serde(default = "default_ep_earnings_bd_post")]
    pub earnings_bd_post: u32,
    #[serde(default = "default_ep_skip_if_unknown_earnings")]
    pub skip_if_unknown_earnings: bool,
    #[serde(default = "default_fomc_enabled")]
    pub fomc_blackout_enabled: bool,
}

impl Default for EpisodicPivotCfg {
    fn default() -> Self {
        Self {
            min_gap_pct: default_ep_min_gap_pct(),
            min_sentiment_abs: default_ep_min_sentiment_abs(),
            min_volume_ratio: default_ep_min_volume_ratio(),
            earnings_bd_pre: default_ep_earnings_bd_pre(),
            earnings_bd_post: default_ep_earnings_bd_post(),
            skip_if_unknown_earnings: default_ep_skip_if_unknown_earnings(),
            fomc_blackout_enabled: default_fomc_enabled(),
        }
    }
}

impl EpisodicPivotCfg {
    pub fn blackout(&self) -> BlackoutPolicy {
        BlackoutPolicy {
            earnings: EarningsPolicy {
                bd_pre: self.earnings_bd_pre,
                bd_post: self.earnings_bd_post,
                skip_if_unknown: self.skip_if_unknown_earnings,
            },
            fomc: FomcPolicy {
                enabled: self.fomc_blackout_enabled,
            },
        }
    }
}

fn default_ep_min_gap_pct() -> f64 {
    0.04
}
fn default_ep_min_sentiment_abs() -> f64 {
    0.15
}
fn default_ep_min_volume_ratio() -> f64 {
    1.0
}
fn default_ep_earnings_bd_pre() -> u32 {
    0
}
fn default_ep_earnings_bd_post() -> u32 {
    0
}
fn default_ep_skip_if_unknown_earnings() -> bool {
    false
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParabolicShortCfg {
    #[serde(default = "default_ps_min_consec_days")]
    pub min_consec_days: u32,
    #[serde(default = "default_ps_min_per_day_move")]
    pub min_per_day_move: f64,
    #[serde(default = "default_ps_min_cumulative_move")]
    pub min_cumulative_move: f64,
    #[serde(default = "default_ps_min_atr_distance")]
    pub min_atr_distance: f64,
    #[serde(default = "default_ps_min_rsi")]
    pub min_rsi: f64,
    /// Phase 5 — parabolic-short is more sensitive to pre-earnings
    /// ramps. Master plan default: 10 BD pre + 1 BD post.
    #[serde(default = "default_ps_earnings_bd_pre")]
    pub earnings_bd_pre: u32,
    #[serde(default = "default_ps_earnings_bd_post")]
    pub earnings_bd_post: u32,
    #[serde(default = "default_ps_skip_if_unknown_earnings")]
    pub skip_if_unknown_earnings: bool,
    #[serde(default = "default_fomc_enabled")]
    pub fomc_blackout_enabled: bool,
}

impl Default for ParabolicShortCfg {
    fn default() -> Self {
        Self {
            min_consec_days: default_ps_min_consec_days(),
            min_per_day_move: default_ps_min_per_day_move(),
            min_cumulative_move: default_ps_min_cumulative_move(),
            min_atr_distance: default_ps_min_atr_distance(),
            min_rsi: default_ps_min_rsi(),
            earnings_bd_pre: default_ps_earnings_bd_pre(),
            earnings_bd_post: default_ps_earnings_bd_post(),
            skip_if_unknown_earnings: default_ps_skip_if_unknown_earnings(),
            fomc_blackout_enabled: default_fomc_enabled(),
        }
    }
}

impl ParabolicShortCfg {
    pub fn blackout(&self) -> BlackoutPolicy {
        BlackoutPolicy {
            earnings: EarningsPolicy {
                bd_pre: self.earnings_bd_pre,
                bd_post: self.earnings_bd_post,
                skip_if_unknown: self.skip_if_unknown_earnings,
            },
            fomc: FomcPolicy {
                enabled: self.fomc_blackout_enabled,
            },
        }
    }
}

fn default_ps_min_consec_days() -> u32 {
    3
}
fn default_ps_min_per_day_move() -> f64 {
    0.05
}
fn default_ps_min_cumulative_move() -> f64 {
    0.40
}
fn default_ps_min_atr_distance() -> f64 {
    2.0
}
fn default_ps_min_rsi() -> f64 {
    80.0
}
fn default_ps_earnings_bd_pre() -> u32 {
    10
}
fn default_ps_earnings_bd_post() -> u32 {
    1
}
fn default_ps_skip_if_unknown_earnings() -> bool {
    true
}
