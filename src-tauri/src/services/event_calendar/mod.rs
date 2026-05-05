// Phase 5 introduces a wide public surface (overrides upsert / clear,
// cache upsert / clear_all, composite force_refresh, accessor helpers
// on the service) that production code will only fully consume once
// the AV upstream wiring lands and the MCP rail exposes the manual-
// override tools. Until then the API surface stays in place so the
// trait seams + tests are exercised; the dead-code allow-list keeps
// pre-commit's `-D warnings` happy without sprinkling per-method
// allow attributes through the file.
#![allow(dead_code, unused_imports)]

//! Phase 5 — `EventCalendarService`: the deterministic event-blackout
//! gate. The runner consults this between detector hit and persistence
//! so detectors don't fire setups inside earnings or FOMC windows
//! they're not designed to handle.
//!
//! Architecture: the service is `Send + Sync + 'static` and constructed
//! once at boot. It composes:
//!   - an `EarningsCalendar` (typically `CompositeEarningsCalendar`
//!     wrapping manual overrides + cache + optional upstream fetcher);
//!   - a `FomcCalendar` loaded from the embedded JSON dataset.
//!
//! The gate is per-detector — `is_blackout` takes a `&BlackoutPolicy`
//! sourced from the detector's config. Episodic-pivot's policy
//! disables earnings (`bd_pre = bd_post = 0`); breakout / parabolic-
//! short opt in.

use std::sync::Arc;

use chrono::{DateTime, Datelike, Duration as ChronoDuration, NaiveDate, Utc};
use thiserror::Error;
use tracing::warn;

use crate::utils::market_calendar::{et_date, et_offset, is_holiday, trading_days_before};

pub mod earnings;
pub mod earnings_store;
pub mod fomc;
pub mod types;

#[cfg(test)]
mod tests;

pub use earnings::{
    CompositeEarningsCalendar, EarningsCalendar, EarningsEntry, EarningsError, NoOpUpstream,
    UpstreamEarningsFetcher,
};
pub use earnings_store::{EarningsCacheStore, EarningsOverridesStore, EarningsRow, OverrideRow};
pub use fomc::{FomcCalendar, FomcError};
pub use types::{
    Blackout, BlackoutConfidence, BlackoutKind, BlackoutPolicy, EarningsPolicy, FomcPolicy,
};

/// Result of a `event_calendar_lookup` call — what the UI shows
/// without needing to know the gate's policy.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EventCalendarLookup {
    pub symbol: String,
    /// Next known earnings date (manual / cache / upstream), or `None`.
    pub next_earnings: Option<EarningsLookup>,
    /// Days from today (ET) to the next FOMC meeting, or `None` when
    /// the dataset has no future entries.
    pub days_to_fomc: Option<i64>,
    /// `true` when the FOMC dataset's last entry is closer than 90
    /// days from today — operator should refresh the JSON.
    pub fomc_dataset_stale: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EarningsLookup {
    pub date: NaiveDate,
    pub confidence: BlackoutConfidence,
    pub source: String,
    /// Trading days from today (ET) to the earnings date. Used by the
    /// UI's "earnings in N BD" copy. `0` when `today >= date`.
    pub trading_days_until: u32,
}

#[derive(Debug, Error)]
pub enum EventCalendarError {
    #[error("earnings: {0}")]
    Earnings(#[from] earnings::EarningsError),
}

pub type Result<T> = std::result::Result<T, EventCalendarError>;

/// The composed event-blackout gate.
pub struct EventCalendarService {
    earnings: Arc<dyn EarningsCalendar>,
    /// Direct handle on the cache so `force_refresh` doesn't have to
    /// downcast through the `EarningsCalendar` trait. `None` for tests
    /// that wire a non-DB-backed earnings impl.
    earnings_cache: Option<Arc<EarningsCacheStore>>,
    fomc: Arc<FomcCalendar>,
}

impl EventCalendarService {
    pub fn new(earnings: Arc<dyn EarningsCalendar>, fomc: Arc<FomcCalendar>) -> Self {
        Self {
            earnings,
            earnings_cache: None,
            fomc,
        }
    }

    /// Wire a direct cache handle so `force_refresh` can clear the
    /// cache without going through the trait. Production constructor
    /// in `lib.rs::run` calls this after assembling the composite.
    pub fn with_cache(mut self, cache: Arc<EarningsCacheStore>) -> Self {
        self.earnings_cache = Some(cache);
        self
    }

    /// Discard cached earnings rows. Wired to the
    /// `event_calendar_force_refresh` Tauri command so the operator
    /// can purge before a morning sweep.
    pub async fn force_refresh(&self) -> Result<()> {
        if let Some(cache) = &self.earnings_cache {
            cache
                .clear_all()
                .await
                .map_err(|e| EventCalendarError::Earnings(EarningsError::Storage(e)))?;
        }
        Ok(())
    }

    /// `Some(Blackout)` if `(symbol, at)` falls inside an event window
    /// per `policy`; `None` otherwise. The two checks compose: an
    /// FOMC blackout returns even for symbols whose earnings policy
    /// is disabled (FOMC is market-wide). The first hit wins —
    /// earnings is checked first because earnings windows span days,
    /// FOMC is hours of one day.
    pub async fn is_blackout(
        &self,
        symbol: &str,
        at: DateTime<Utc>,
        policy: &BlackoutPolicy,
    ) -> Result<Option<Blackout>> {
        // 1. Earnings: only consult if the detector has a non-zero window.
        if !policy.earnings.is_disabled() {
            let entry = self.earnings.next_earnings_date(symbol, at).await?;
            match entry {
                Some(e) => {
                    if let Some(b) = earnings_blackout(&e, at, &policy.earnings) {
                        return Ok(Some(b));
                    }
                }
                None => {
                    if policy.earnings.skip_if_unknown {
                        // No source has earnings for this symbol — the
                        // policy says "skip when unknown", so synth a
                        // sentinel blackout that tells the runner why.
                        return Ok(Some(unknown_earnings_blackout(at)));
                    }
                }
            }
        }

        // 2. FOMC: short window, market-wide, applies to every detector
        //    that hasn't opted out.
        if policy.fomc.enabled {
            if self.fomc.is_stale(at) {
                warn!(
                    "fomc dataset is stale (last meeting < 90 days from now) — \
                     update src-tauri/data/fomc_dates.json"
                );
            }
            if let Some(b) = self.fomc.lookup(at) {
                return Ok(Some(b));
            }
        }

        Ok(None)
    }

    pub async fn lookup(&self, symbol: &str, at: DateTime<Utc>) -> Result<EventCalendarLookup> {
        let earnings = self.earnings.next_earnings_date(symbol, at).await?;
        let next_earnings = earnings.map(|e| {
            let today = et_date(at);
            let trading_days_until = trading_days_between_et(today, e.date);
            EarningsLookup {
                date: e.date,
                confidence: e.confidence,
                source: e.source,
                trading_days_until,
            }
        });
        Ok(EventCalendarLookup {
            symbol: symbol.trim().to_uppercase(),
            next_earnings,
            days_to_fomc: self.fomc.days_to_next(at),
            fomc_dataset_stale: self.fomc.is_stale(at),
        })
    }

    pub fn earnings(&self) -> &Arc<dyn EarningsCalendar> {
        &self.earnings
    }

    pub fn fomc(&self) -> &Arc<FomcCalendar> {
        &self.fomc
    }
}

/// `Some(Blackout)` when `at` falls inside the earnings window anchored
/// at `entry.date`.
fn earnings_blackout(
    entry: &EarningsEntry,
    at: DateTime<Utc>,
    policy: &EarningsPolicy,
) -> Option<Blackout> {
    if policy.is_disabled() {
        return None;
    }

    let pivot = entry.date;
    // Window: [start, end). `start` = midnight ET, `bd_pre` BDs before
    // pivot. `end` = midnight ET the day after `bd_post`-th BD after
    // pivot. We use midnight-to-midnight ET to keep the window stable
    // across timezone shifts; the integration test pins now() during RTH.
    let start_date = trading_days_before(pivot, policy.bd_pre);
    let end_date = if policy.bd_post == 0 {
        // bd_post = 0 → window ends at the close of the pivot day,
        // i.e. midnight ET of the day after the pivot.
        next_day(pivot)
    } else {
        next_day(crate::utils::market_calendar::trading_days_after(
            pivot,
            policy.bd_post,
        ))
    };
    let start = et_midnight_utc(start_date);
    let end = et_midnight_utc(end_date);
    if at < start || at >= end {
        return None;
    }
    let confidence = entry.confidence;
    Some(Blackout {
        kind: BlackoutKind::Earnings,
        start,
        end,
        pivot_date: pivot,
        reason: format!(
            "earnings on {} ({}; window {} BD pre / {} BD post)",
            pivot,
            confidence.as_str(),
            policy.bd_pre,
            policy.bd_post,
        ),
        source: entry.source.clone(),
        confidence,
    })
}

fn unknown_earnings_blackout(at: DateTime<Utc>) -> Blackout {
    let today = et_date(at);
    Blackout {
        kind: BlackoutKind::Earnings,
        start: et_midnight_utc(today),
        end: et_midnight_utc(next_day(today)),
        pivot_date: today,
        reason: "earnings date unknown for symbol; policy skip_if_unknown=true".to_string(),
        source: "unknown".to_string(),
        confidence: BlackoutConfidence::Estimated,
    }
}

fn next_day(date: NaiveDate) -> NaiveDate {
    date.succ_opt().expect("date arithmetic does not overflow")
}

fn et_midnight_utc(date: NaiveDate) -> DateTime<Utc> {
    use chrono::{NaiveTime, TimeZone};
    let naive = date.and_time(NaiveTime::from_hms_opt(0, 0, 0).expect("valid"));
    et_offset()
        .from_local_datetime(&naive)
        .single()
        .expect("ET fixed offset")
        .with_timezone(&Utc)
}

/// Wrapper around `market_calendar::trading_days_between` that returns
/// `0` when either bound is in the past. Pulled out so the lookup
/// function reads cleanly.
fn trading_days_between_et(today: NaiveDate, target: NaiveDate) -> u32 {
    if target <= today {
        return 0;
    }
    crate::utils::market_calendar::trading_days_between(today, target)
}

// Suppress dead-code warning for helpers that may only be exercised in
// tests when callers bind to is_blackout.
#[allow(dead_code)]
fn day_of_week(d: NaiveDate) -> chrono::Weekday {
    d.weekday()
}

#[allow(dead_code)]
fn is_holiday_check(d: NaiveDate) -> bool {
    is_holiday(d)
}

#[allow(dead_code)]
fn one_day() -> ChronoDuration {
    ChronoDuration::days(1)
}
