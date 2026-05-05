//! Shared types for the event-blackout gate.
//!
//! `Blackout` is what
//! [`crate::services::event_calendar::EventCalendarService::is_blackout`]
//! returns when a `(symbol, at)` lookup falls inside an event window.
//! Persisted on `setups.skip_window_json` so the UI can render
//! "skipped: earnings in 3 BD" without a second query.
//!
//! `BlackoutPolicy` is per-detector and lives in
//! `strategies::config` — the gate only consumes a `&BlackoutPolicy`,
//! never owns one.

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

/// What kind of event window a `Blackout` describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlackoutKind {
    Earnings,
    Fomc,
}

impl BlackoutKind {
    pub fn as_str(self) -> &'static str {
        match self {
            BlackoutKind::Earnings => "earnings",
            BlackoutKind::Fomc => "fomc",
        }
    }
}

/// Confidence the blackout's pivot date carries. Earnings dates from
/// AV are estimates until the issuer announces; FOMC dates from the
/// hardcoded JSON are confirmed by the Fed schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlackoutConfidence {
    Estimated,
    Confirmed,
}

impl BlackoutConfidence {
    pub fn as_str(self) -> &'static str {
        match self {
            BlackoutConfidence::Estimated => "estimated",
            BlackoutConfidence::Confirmed => "confirmed",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "estimated" => Some(BlackoutConfidence::Estimated),
            "confirmed" => Some(BlackoutConfidence::Confirmed),
            _ => None,
        }
    }
}

/// Result of a positive `is_blackout` lookup. Persisted as JSON on the
/// gated `setups` row so the UI and the audit path can reproduce why
/// the gate fired without re-querying the calendar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Blackout {
    pub kind: BlackoutKind,
    /// First UTC instant inside the window (window-start).
    pub start: DateTime<Utc>,
    /// First UTC instant *after* the window (window-end exclusive).
    /// Lookups satisfy `start <= at < end`.
    pub end: DateTime<Utc>,
    /// The pivot date the window is anchored to (next earnings date,
    /// FOMC meeting date). Stored separately from `start`/`end` so the
    /// UI can render "earnings 2026-05-08" without re-deriving from the
    /// window bounds.
    pub pivot_date: NaiveDate,
    /// Short human-readable reason for the audit trail.
    pub reason: String,
    /// Where the pivot date came from (`"alpha_vantage"`,
    /// `"manual"`, `"fomc_dataset"`).
    pub source: String,
    /// Estimate vs confirmed.
    pub confidence: BlackoutConfidence,
}

/// Per-detector earnings policy. Lives on each detector's config (e.g.
/// `BreakoutCfg`); the gate borrows a reference rather than owning a
/// copy so settings reloads pick up immediately.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EarningsPolicy {
    /// Trading days before the next earnings date the window covers.
    /// `0` disables the earnings gate for this detector — episodic-pivot
    /// is the canonical user (it's literally meant to trade earnings
    /// gaps). Default 5 (master plan).
    pub bd_pre: u32,
    /// Trading days after the next earnings date the window covers.
    /// Default 1 (master plan).
    pub bd_post: u32,
    /// What to do when no earnings date is known for the symbol. `true`
    /// is conservative (skip; safer for breakout); `false` lets the
    /// detector fire (right for episodic-pivot which trades on news).
    pub skip_if_unknown: bool,
}

impl Default for EarningsPolicy {
    fn default() -> Self {
        Self {
            bd_pre: 5,
            bd_post: 1,
            skip_if_unknown: true,
        }
    }
}

impl EarningsPolicy {
    /// `true` if this detector should not consult the earnings
    /// calendar at all (`bd_pre == 0 && bd_post == 0`). Episodic-pivot
    /// uses this — earnings news *is* the trade.
    pub fn is_disabled(&self) -> bool {
        self.bd_pre == 0 && self.bd_post == 0
    }
}

/// Per-detector FOMC policy. The master plan committed: day-of FOMC,
/// 14:00 ET → close. Configurable per detector so a future detector
/// can opt out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FomcPolicy {
    /// `true` for "honor the FOMC blackout"; `false` to opt out
    /// entirely.
    pub enabled: bool,
}

impl Default for FomcPolicy {
    fn default() -> Self {
        Self { enabled: true }
    }
}

/// Bundle of policies a detector publishes to the gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct BlackoutPolicy {
    pub earnings: EarningsPolicy,
    pub fomc: FomcPolicy,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn earnings_disabled_when_zero_pre_post() {
        let p = EarningsPolicy {
            bd_pre: 0,
            bd_post: 0,
            skip_if_unknown: false,
        };
        assert!(p.is_disabled());

        let p = EarningsPolicy {
            bd_pre: 5,
            bd_post: 1,
            skip_if_unknown: true,
        };
        assert!(!p.is_disabled());
    }

    #[test]
    fn confidence_round_trip() {
        for c in [BlackoutConfidence::Estimated, BlackoutConfidence::Confirmed] {
            assert_eq!(BlackoutConfidence::parse(c.as_str()), Some(c));
        }
        assert_eq!(BlackoutConfidence::parse("garbage"), None);
    }
}
