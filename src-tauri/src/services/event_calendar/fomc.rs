//! FOMC calendar — hardcoded next-N-months meeting dates loaded from
//! `data/fomc_dates.json`. The master plan committed: day-of FOMC,
//! 14:00 ET → close. Phase 5 ships that exact window; widening to
//! T-1 close → T+1 09:35 is gated on P6 backtest evidence.
//!
//! We embed the JSON at compile time (`include_str!`) so the runtime
//! has no on-disk dependency. Updates ship as code changes; the
//! freshness warning surfaces in `EventCalendarService::is_blackout`
//! when the last entry is fewer than 90 days from `now`.

use chrono::{DateTime, Datelike, NaiveDate, Utc};
use serde::Deserialize;
use thiserror::Error;

use crate::utils::market_calendar::{et_date, et_offset};

use super::types::{Blackout, BlackoutConfidence, BlackoutKind};

/// Embedded FOMC dataset. Compiled in so a packaged release does not
/// need to ship `data/fomc_dates.json` separately. Update by editing
/// the JSON file at the repo root and recompiling.
const FOMC_DATES_JSON: &str = include_str!("../../../data/fomc_dates.json");

/// Window-start hour in ET (14:00 ET — when the rate statement
/// historically drops and the vol expansion fires).
const WINDOW_START_HOUR_ET: u32 = 14;
/// Window-end hour in ET (16:00 ET — RTH close).
const WINDOW_END_HOUR_ET: u32 = 16;
/// Warn when the last meeting in the dataset is closer than this many
/// days from `now`. The runtime continues to function past this point
/// — a warning surfaces in logs and (eventually) in the UI banner.
const FRESHNESS_WARN_DAYS: i64 = 90;

#[derive(Debug, Error)]
pub enum FomcError {
    #[error("fomc dataset parse error: {0}")]
    Parse(String),
}

#[derive(Deserialize)]
struct FomcDataset {
    #[serde(default)]
    meetings: Vec<String>,
}

/// Loaded FOMC calendar. Cheap to clone (`Vec<NaiveDate>`).
#[derive(Debug, Clone)]
pub struct FomcCalendar {
    meetings: Vec<NaiveDate>,
}

impl FomcCalendar {
    /// Load the embedded dataset. Errors if the JSON is malformed —
    /// callers should panic at startup so a corrupted dataset never
    /// silently disables the gate.
    pub fn from_embedded() -> Result<Self, FomcError> {
        Self::from_json(FOMC_DATES_JSON)
    }

    pub fn from_json(json: &str) -> Result<Self, FomcError> {
        let parsed: FomcDataset =
            serde_json::from_str(json).map_err(|e| FomcError::Parse(e.to_string()))?;
        let mut meetings = Vec::with_capacity(parsed.meetings.len());
        for s in parsed.meetings {
            let d = NaiveDate::parse_from_str(&s, "%Y-%m-%d")
                .map_err(|e| FomcError::Parse(format!("invalid date '{s}': {e}")))?;
            meetings.push(d);
        }
        meetings.sort();
        meetings.dedup();
        Ok(Self { meetings })
    }

    /// Used by tests to construct a calendar from explicit dates.
    pub fn from_dates(mut meetings: Vec<NaiveDate>) -> Self {
        meetings.sort();
        meetings.dedup();
        Self { meetings }
    }

    /// `true` when `at` falls inside the FOMC blackout window for some
    /// meeting in the dataset. Window is `[14:00 ET, 16:00 ET)` on the
    /// meeting date. Returns the matching `Blackout` so the caller can
    /// audit the source.
    pub fn lookup(&self, at: DateTime<Utc>) -> Option<Blackout> {
        let today = et_date(at);
        // Binary-search the sorted list — linear scan would also be
        // fine at N=16 but binary search keeps the gate cheap as the
        // dataset grows.
        let idx = self.meetings.binary_search(&today).ok()?;
        let pivot = self.meetings[idx];
        let start = et_local_to_utc(pivot, WINDOW_START_HOUR_ET, 0);
        let end = et_local_to_utc(pivot, WINDOW_END_HOUR_ET, 0);
        if at < start || at >= end {
            return None;
        }
        Some(Blackout {
            kind: BlackoutKind::Fomc,
            start,
            end,
            pivot_date: pivot,
            reason: format!("FOMC rate decision {pivot}: 14:00–16:00 ET vol-expansion blackout"),
            source: "fomc_dataset".to_string(),
            confidence: BlackoutConfidence::Confirmed,
        })
    }

    /// Last meeting date in the dataset, used by the freshness check.
    pub fn last_meeting(&self) -> Option<NaiveDate> {
        self.meetings.last().copied()
    }

    /// `true` when the dataset's last entry is closer than
    /// `FRESHNESS_WARN_DAYS` from `now` (or the dataset is empty).
    /// Callers should log a warning so the operator knows to refresh.
    pub fn is_stale(&self, now: DateTime<Utc>) -> bool {
        match self.last_meeting() {
            Some(last) => {
                let today = et_date(now);
                let delta = (last - today).num_days();
                delta < FRESHNESS_WARN_DAYS
            }
            None => true,
        }
    }

    /// Days until the next meeting on or after `now`. `None` when no
    /// future meeting is in the dataset. Used by `event_calendar_lookup`
    /// for the UI's "FOMC in N days" copy.
    pub fn days_to_next(&self, now: DateTime<Utc>) -> Option<i64> {
        let today = et_date(now);
        self.meetings
            .iter()
            .find(|d| **d >= today)
            .map(|d| (*d - today).num_days())
    }
}

fn et_local_to_utc(date: NaiveDate, h: u32, m: u32) -> DateTime<Utc> {
    use chrono::{NaiveTime, TimeZone};
    let naive = date.and_time(NaiveTime::from_hms_opt(h, m, 0).expect("valid time"));
    et_offset()
        .from_local_datetime(&naive)
        .single()
        .expect("ET is fixed offset")
        .with_timezone(&Utc)
}

// Suppress dead-code warning for the day-of-year helper in case the
// future widening path uses it; harmless to keep.
#[allow(dead_code)]
fn day_of_year(date: NaiveDate) -> u32 {
    date.ordinal()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at_et(d: NaiveDate, h: u32, m: u32) -> DateTime<Utc> {
        et_local_to_utc(d, h, m)
    }

    fn cal() -> FomcCalendar {
        FomcCalendar::from_dates(vec![
            NaiveDate::from_ymd_opt(2026, 5, 6).unwrap(),
            NaiveDate::from_ymd_opt(2026, 6, 17).unwrap(),
        ])
    }

    #[test]
    fn embedded_dataset_loads() {
        let cal = FomcCalendar::from_embedded().expect("embedded dataset must parse");
        assert!(!cal.meetings.is_empty(), "dataset must have entries");
        // Sorted invariant
        let sorted: Vec<_> = {
            let mut v = cal.meetings.clone();
            v.sort();
            v
        };
        assert_eq!(cal.meetings, sorted);
    }

    #[test]
    fn returns_blackout_at_14_00_et_on_meeting_day() {
        let c = cal();
        let at = at_et(NaiveDate::from_ymd_opt(2026, 5, 6).unwrap(), 14, 0);
        let b = c.lookup(at).expect("must hit blackout");
        assert_eq!(b.kind, BlackoutKind::Fomc);
        assert_eq!(b.confidence, BlackoutConfidence::Confirmed);
        assert_eq!(b.source, "fomc_dataset");
    }

    #[test]
    fn returns_blackout_at_15_30_et_on_meeting_day() {
        let c = cal();
        let at = at_et(NaiveDate::from_ymd_opt(2026, 5, 6).unwrap(), 15, 30);
        assert!(c.lookup(at).is_some());
    }

    #[test]
    fn no_blackout_at_13_59_et_on_meeting_day() {
        let c = cal();
        let at = at_et(NaiveDate::from_ymd_opt(2026, 5, 6).unwrap(), 13, 59);
        assert!(
            c.lookup(at).is_none(),
            "13:59 ET is *before* the 14:00 ET window-start"
        );
    }

    #[test]
    fn no_blackout_at_16_00_et_close_on_meeting_day() {
        let c = cal();
        let at = at_et(NaiveDate::from_ymd_opt(2026, 5, 6).unwrap(), 16, 0);
        assert!(
            c.lookup(at).is_none(),
            "16:00 ET is the exclusive upper bound — window closes at the open"
        );
    }

    #[test]
    fn no_blackout_off_meeting_day() {
        let c = cal();
        let at = at_et(NaiveDate::from_ymd_opt(2026, 5, 7).unwrap(), 14, 0);
        assert!(c.lookup(at).is_none());
    }

    #[test]
    fn days_to_next_counts_to_first_future_meeting() {
        let c = cal();
        let now = Utc.with_ymd_and_hms(2026, 5, 6, 9, 30, 0).unwrap();
        assert_eq!(c.days_to_next(now), Some(0));
        let later = Utc.with_ymd_and_hms(2026, 5, 7, 9, 30, 0).unwrap();
        assert_eq!(c.days_to_next(later), Some(41));
    }

    #[test]
    fn empty_calendar_is_stale() {
        let c = FomcCalendar::from_dates(Vec::new());
        let now = Utc.with_ymd_and_hms(2026, 5, 6, 0, 0, 0).unwrap();
        assert!(c.is_stale(now));
    }
}
