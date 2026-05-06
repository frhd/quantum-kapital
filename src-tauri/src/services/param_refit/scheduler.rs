//! Phase 10 — monthly cron scheduler for `ParamRefitService`. Wakes
//! at the next "after market close on the last trading day of the
//! month" tick, runs `run_monthly`, sleeps until the next month's
//! tick. Mirrors the pattern from
//! `services/intraday_scheduler` + `services/social_sentiment_scheduler`:
//! a `tokio::task` that owns the `Arc<Service>` and runs until its
//! handle is dropped.
//!
//! Cadence rule (master-plan committed): "schedule for off-hours"
//! ⇒ run at 17:00 ET on the last business day of each calendar
//! month. The scheduler computes the next tick deterministically
//! from the existing `market_calendar` helpers.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Datelike, NaiveDate, NaiveTime, TimeZone, Utc};
use tracing::{info, warn};

use crate::utils::market_calendar;

use super::vintage_store::LockSource;
use super::ParamRefitService;

/// Hour after market close at which to run the monthly refit.
/// 17:00 ET ⇒ 1 hour after the standard 16:00 ET close. Far enough
/// from close that EOD scheduler doesn't compete; before midnight
/// so the run finishes within the same trading day's logs.
const REFIT_HOUR_ET: u32 = 17;

/// Minimum sleep slice. Avoids busy-spinning when the next-tick
/// computation lands inside the past (drift). Fine to wake early
/// — the loop re-checks before running.
const MIN_SLEEP_SECS: u64 = 60;

pub struct MonthlyRefitScheduler {
    service: Arc<ParamRefitService>,
}

impl MonthlyRefitScheduler {
    pub fn new(service: Arc<ParamRefitService>) -> Self {
        Self { service }
    }

    /// Spawn the scheduler loop. Returns a `tokio::task::JoinHandle`
    /// the caller drops to stop the loop. Mirrors the
    /// social-sentiment scheduler's `spawn` pattern.
    pub fn spawn(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            self.run_loop().await;
        })
    }

    async fn run_loop(self: Arc<Self>) {
        loop {
            let now = Utc::now();
            let next = next_refit_at(now);
            let sleep_for = next.signed_duration_since(now).num_seconds().max(0) as u64;
            let sleep_for = sleep_for.max(MIN_SLEEP_SECS);
            info!(
                "param_refit_scheduler: next refit at {} ({}s away)",
                next.to_rfc3339(),
                sleep_for
            );
            tokio::time::sleep(Duration::from_secs(sleep_for)).await;
            // Re-check: if the system clock jumped backwards or
            // we woke early, sleep a bit more.
            let now = Utc::now();
            if now < next {
                let remainder = next.signed_duration_since(now).num_seconds().max(0) as u64;
                if remainder > 0 {
                    tokio::time::sleep(Duration::from_secs(remainder)).await;
                }
            }
            match self.service.run_monthly(LockSource::Cron).await {
                Ok(report) => {
                    info!(
                        "param_refit_scheduler: monthly refit complete: {} detector(s) processed",
                        report.outcomes.len()
                    );
                    for outcome in &report.outcomes {
                        info!(
                            "  - {}: status={} note={}",
                            outcome.detector,
                            outcome.status.as_str(),
                            outcome.note
                        );
                    }
                }
                Err(e) => {
                    warn!("param_refit_scheduler: monthly refit failed: {e}");
                }
            }
        }
    }
}

/// Compute the next 17:00 ET on the last trading day of the
/// current or next calendar month, whichever is the next future
/// instant. Pure: takes `now` so tests can pin it.
pub fn next_refit_at(now: DateTime<Utc>) -> DateTime<Utc> {
    let et_today = market_calendar::et_date(now);
    // Try this month's last trading day first.
    let this_month_target = month_refit_at(et_today.year(), et_today.month());
    if this_month_target > now {
        return this_month_target;
    }
    // Roll to next month.
    let (next_year, next_month) = if et_today.month() == 12 {
        (et_today.year() + 1, 1)
    } else {
        (et_today.year(), et_today.month() + 1)
    };
    month_refit_at(next_year, next_month)
}

fn month_refit_at(year: i32, month: u32) -> DateTime<Utc> {
    let last_trading_day = last_trading_day_of_month(year, month);
    let naive = last_trading_day
        .and_time(NaiveTime::from_hms_opt(REFIT_HOUR_ET, 0, 0).expect("valid time"));
    market_calendar::et_offset()
        .from_local_datetime(&naive)
        .single()
        .expect("ET fixed offset")
        .with_timezone(&Utc)
}

pub use crate::utils::market_calendar::last_trading_day_of_month;

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn last_trading_day_handles_weekend_endings() {
        // May 2026 ends on Sunday, May 31 → expect Friday May 29.
        let d = last_trading_day_of_month(2026, 5);
        assert_eq!(d, NaiveDate::from_ymd_opt(2026, 5, 29).unwrap());
    }

    #[test]
    fn last_trading_day_skips_holidays() {
        // December 2026: Christmas Day (Dec 25) is a Friday holiday.
        // Last trading day should be Dec 31 (Thursday).
        let d = last_trading_day_of_month(2026, 12);
        assert_eq!(d, NaiveDate::from_ymd_opt(2026, 12, 31).unwrap());
    }

    #[test]
    fn next_refit_rolls_to_next_month_after_current_month_target() {
        // Pin `now` to the middle of June 2026.
        let now = Utc.with_ymd_and_hms(2026, 6, 15, 14, 0, 0).unwrap();
        let next = next_refit_at(now);
        // Should be the last trading day of June 2026.
        let last_jun = last_trading_day_of_month(2026, 6);
        let expected_date_et = next
            .with_timezone(&market_calendar::et_offset())
            .date_naive();
        assert_eq!(expected_date_et, last_jun);
    }

    #[test]
    fn next_refit_after_month_end_jumps_to_next_month() {
        // Pin `now` to after this month's refit moment (Jul 31 18:00 ET).
        let last_jul = last_trading_day_of_month(2026, 7);
        let now_et = market_calendar::et_offset()
            .from_local_datetime(&last_jul.and_hms_opt(18, 0, 0).unwrap())
            .single()
            .unwrap();
        let now = now_et.with_timezone(&Utc);
        let next = next_refit_at(now);
        let next_date_et = next
            .with_timezone(&market_calendar::et_offset())
            .date_naive();
        let last_aug = last_trading_day_of_month(2026, 8);
        assert_eq!(next_date_et, last_aug);
    }

    #[test]
    fn refit_at_lands_at_17_et() {
        use chrono::Timelike;
        let target = month_refit_at(2026, 5);
        let et = target.with_timezone(&market_calendar::et_offset());
        assert_eq!(et.hour(), REFIT_HOUR_ET);
        assert_eq!(et.minute(), 0);
    }
}
