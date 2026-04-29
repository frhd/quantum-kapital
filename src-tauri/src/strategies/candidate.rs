use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::ibkr::types::{BarSize, StrategyTag};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Long,
    Short,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TargetLevel {
    pub label: String,
    pub price: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetupCandidate {
    pub strategy: &'static str,
    pub tag: StrategyTag,
    pub direction: Direction,
    pub conviction_signal: f64,
    pub trigger_price: f64,
    pub stop_price: f64,
    pub targets: Vec<TargetLevel>,
    pub raw_signals: serde_json::Value,
    pub timeframe: BarSize,
    pub detected_at: DateTime<Utc>,
}

pub fn targets_for_risk_profile(
    direction: Direction,
    trigger: f64,
    stop: f64,
) -> Result<Vec<TargetLevel>, &'static str> {
    if !trigger.is_finite() || !stop.is_finite() {
        return Err("trigger and stop must be finite");
    }
    let risk = (trigger - stop).abs();
    if risk == 0.0 {
        return Err("trigger and stop are equal — risk distance is zero");
    }
    let signed = match direction {
        Direction::Long => 1.0,
        Direction::Short => -1.0,
    };
    Ok(vec![
        TargetLevel {
            label: "2R".to_string(),
            price: trigger + signed * 2.0 * risk,
        },
        TargetLevel {
            label: "3R".to_string(),
            price: trigger + signed * 3.0 * risk,
        },
    ])
}
