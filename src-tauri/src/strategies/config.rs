//! Tunable thresholds for the production detectors.
//!
//! Defaults mirror the constants encoded in Phases 07–09. Settings from
//! `~/.config/quantum-kapital/settings.json` override these without
//! recompiling; missing sections fall back to defaults so older settings
//! files keep working.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DetectorsConfig {
    #[serde(default)]
    pub breakout: BreakoutCfg,
    #[serde(default)]
    pub episodic_pivot: EpisodicPivotCfg,
    #[serde(default)]
    pub parabolic_short: ParabolicShortCfg,
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
}

impl Default for BreakoutCfg {
    fn default() -> Self {
        Self {
            lookback_days: default_breakout_lookback_days(),
            volume_multiple: default_breakout_volume_multiple(),
            rsi_ceiling: default_breakout_rsi_ceiling(),
            atr_period: default_breakout_atr_period(),
            swing_low_period: default_breakout_swing_low_period(),
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpisodicPivotCfg {
    #[serde(default = "default_ep_min_gap_pct")]
    pub min_gap_pct: f64,
    #[serde(default = "default_ep_min_sentiment_abs")]
    pub min_sentiment_abs: f64,
    #[serde(default = "default_ep_min_volume_ratio")]
    pub min_volume_ratio: f64,
}

impl Default for EpisodicPivotCfg {
    fn default() -> Self {
        Self {
            min_gap_pct: default_ep_min_gap_pct(),
            min_sentiment_abs: default_ep_min_sentiment_abs(),
            min_volume_ratio: default_ep_min_volume_ratio(),
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
}

impl Default for ParabolicShortCfg {
    fn default() -> Self {
        Self {
            min_consec_days: default_ps_min_consec_days(),
            min_per_day_move: default_ps_min_per_day_move(),
            min_cumulative_move: default_ps_min_cumulative_move(),
            min_atr_distance: default_ps_min_atr_distance(),
            min_rsi: default_ps_min_rsi(),
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
