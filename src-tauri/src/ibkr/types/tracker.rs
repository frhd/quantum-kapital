//! Phase 04 — Tracker watchlist domain types.
//!
//! Persisted via `services::tracker_service::TrackerService` against the
//! `tracked_tickers` table. The status state machine is intentionally not
//! enforced here — Phase 04 stores transitions verbatim and Phase 12 will
//! add the validator on top.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackerSource {
    Scanner,
    Manual,
    News,
}

impl TrackerSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            TrackerSource::Scanner => "scanner",
            TrackerSource::Manual => "manual",
            TrackerSource::News => "news",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "scanner" => Some(TrackerSource::Scanner),
            "manual" => Some(TrackerSource::Manual),
            "news" => Some(TrackerSource::News),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackerStatus {
    Watching,
    InPlay,
    SetupActive,
    CoolDown,
}

impl TrackerStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            TrackerStatus::Watching => "watching",
            TrackerStatus::InPlay => "in_play",
            TrackerStatus::SetupActive => "setup_active",
            TrackerStatus::CoolDown => "cool_down",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "watching" => Some(TrackerStatus::Watching),
            "in_play" => Some(TrackerStatus::InPlay),
            "setup_active" => Some(TrackerStatus::SetupActive),
            "cool_down" => Some(TrackerStatus::CoolDown),
            _ => None,
        }
    }
}

/// Strategy tag attached to a tracked ticker. Built-in variants serialize
/// as snake_case strings; `Custom(s)` serializes as `s` verbatim. A custom
/// label that happens to collide with a built-in name (`"breakout"`) will
/// round-trip as the built-in — by design, since they refer to the same
/// concept and we don't want two equivalent representations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StrategyTag {
    Breakout,
    EpisodicPivot,
    ParabolicShort,
    Custom(String),
}

impl StrategyTag {
    pub fn as_str(&self) -> &str {
        match self {
            StrategyTag::Breakout => "breakout",
            StrategyTag::EpisodicPivot => "episodic_pivot",
            StrategyTag::ParabolicShort => "parabolic_short",
            StrategyTag::Custom(s) => s.as_str(),
        }
    }
}

impl Serialize for StrategyTag {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for StrategyTag {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        let s = String::deserialize(de)?;
        Ok(match s.as_str() {
            "breakout" => StrategyTag::Breakout,
            "episodic_pivot" => StrategyTag::EpisodicPivot,
            "parabolic_short" => StrategyTag::ParabolicShort,
            _ => StrategyTag::Custom(s),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrackedTicker {
    pub symbol: String,
    pub source: TrackerSource,
    pub source_meta: Option<serde_json::Value>,
    pub status: TrackerStatus,
    pub tags: Vec<StrategyTag>,
    pub notes: Option<String>,
    pub added_at: DateTime<Utc>,
    pub last_checked_at: Option<DateTime<Utc>>,
    pub in_play_until: Option<DateTime<Utc>>,
    pub cool_down_until: Option<DateTime<Utc>>,
}

/// Lifecycle of a persisted strategy setup. Phase 10 only writes
/// `Active` rows; `Invalidated` and `Completed` are reserved for the
/// status state machine in Phase 12 and the LLM decay-watcher in
/// Phase 18.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SetupStatus {
    Active,
    Invalidated,
    Completed,
}

impl SetupStatus {
    #[allow(dead_code)]
    pub fn as_str(&self) -> &'static str {
        match self {
            SetupStatus::Active => "active",
            SetupStatus::Invalidated => "invalidated",
            SetupStatus::Completed => "completed",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "active" => Some(SetupStatus::Active),
            "invalidated" => Some(SetupStatus::Invalidated),
            "completed" => Some(SetupStatus::Completed),
            _ => None,
        }
    }
}

/// Persisted strategy setup row, mirroring the `setups` table. The
/// `direction` and `targets` types are owned by the strategies module
/// so the persistence layer and the detector framework agree on a
/// single representation; Phase 10 introduces this shared shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Setup {
    pub id: i64,
    pub symbol: String,
    pub strategy: String,
    pub direction: crate::strategies::Direction,
    pub detected_at: DateTime<Utc>,
    pub trigger_price: f64,
    pub stop_price: f64,
    pub targets: Vec<crate::strategies::TargetLevel>,
    pub raw_signals: serde_json::Value,
    pub thesis: Option<String>,
    pub status: SetupStatus,
    pub invalidated_at: Option<DateTime<Utc>>,
    pub invalidation_reason: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracker_source_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&TrackerSource::Scanner).unwrap(),
            "\"scanner\""
        );
        let parsed: TrackerSource = serde_json::from_str("\"news\"").unwrap();
        assert_eq!(parsed, TrackerSource::News);
    }

    #[test]
    fn tracker_status_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&TrackerStatus::SetupActive).unwrap(),
            "\"setup_active\""
        );
        let parsed: TrackerStatus = serde_json::from_str("\"cool_down\"").unwrap();
        assert_eq!(parsed, TrackerStatus::CoolDown);
    }

    #[test]
    fn strategy_tag_builtin_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&StrategyTag::EpisodicPivot).unwrap(),
            "\"episodic_pivot\""
        );
        let parsed: StrategyTag = serde_json::from_str("\"breakout\"").unwrap();
        assert_eq!(parsed, StrategyTag::Breakout);
    }

    #[test]
    fn strategy_tag_custom_round_trips() {
        let tag = StrategyTag::Custom("squeeze".to_string());
        assert_eq!(serde_json::to_string(&tag).unwrap(), "\"squeeze\"");
        let parsed: StrategyTag = serde_json::from_str("\"squeeze\"").unwrap();
        assert_eq!(parsed, StrategyTag::Custom("squeeze".to_string()));
    }

    #[test]
    fn strategy_tag_array_round_trips() {
        let tags = vec![
            StrategyTag::Breakout,
            StrategyTag::Custom("squeeze".to_string()),
        ];
        let s = serde_json::to_string(&tags).unwrap();
        let parsed: Vec<StrategyTag> = serde_json::from_str(&s).unwrap();
        assert_eq!(parsed, tags);
    }
}
