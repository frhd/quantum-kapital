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

/// Phase 21 — kinds of `alerts` rows the tracker pipeline records. The
/// four kinds map 1:1 to the events the frontend AlertFeed surfaces:
/// detector hit (`Detected`), state-machine invalidation (`Invalidated`),
/// target hit on a completed setup (`TargetHit`), and a thesis update
/// from the LLM pipeline (`ThesisChanged`). Storage encodes each as a
/// snake_case string in the `alerts.kind` column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertKind {
    Detected,
    Invalidated,
    TargetHit,
    ThesisChanged,
}

impl AlertKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            AlertKind::Detected => "detected",
            AlertKind::Invalidated => "invalidated",
            AlertKind::TargetHit => "target_hit",
            AlertKind::ThesisChanged => "thesis_changed",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "detected" => Some(AlertKind::Detected),
            "invalidated" => Some(AlertKind::Invalidated),
            "target_hit" => Some(AlertKind::TargetHit),
            "thesis_changed" => Some(AlertKind::ThesisChanged),
            _ => None,
        }
    }
}

/// Persisted alert row mirroring the `alerts` table. `payload` is the
/// event-specific JSON body; the frontend reads `payload.symbol` to wire
/// row clicks to the analysis tab.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Alert {
    pub id: i64,
    pub setup_id: i64,
    pub kind: AlertKind,
    pub fired_at: DateTime<Utc>,
    pub payload: serde_json::Value,
    pub seen: bool,
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
    /// Phase 17 — full structured thesis JSON (markdown + conviction +
    /// invalidation_levels + risk_notes). Markdown also stays in `thesis`
    /// for legacy callers.
    pub thesis_json: Option<serde_json::Value>,
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
    fn alert_kind_round_trips_snake_case() {
        for k in [
            AlertKind::Detected,
            AlertKind::Invalidated,
            AlertKind::TargetHit,
            AlertKind::ThesisChanged,
        ] {
            let s = serde_json::to_string(&k).unwrap();
            let parsed: AlertKind = serde_json::from_str(&s).unwrap();
            assert_eq!(parsed, k);
            assert_eq!(s.trim_matches('"'), k.as_str());
        }
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
