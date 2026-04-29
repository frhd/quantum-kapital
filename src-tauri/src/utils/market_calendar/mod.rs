//! US equity market calendar helpers.
//!
//! TODO(EST/EDT): Eastern time is hardcoded to EST (UTC-5). DST is intentionally
//! skipped for the MVP — the EOD sweep at 16:05 ET and the 5-minute intraday
//! tick are coarse enough that an hour drift during DST is harmless. Revisit
//! only if a real bug surfaces.

// The full surface is exposed for Phases 13/14 (EOD/intraday schedulers); some
// helpers have no caller yet but are part of the public API contract.
#![allow(dead_code)]

mod holidays;

#[cfg(test)]
mod tests;

use chrono::{DateTime, Datelike, FixedOffset, NaiveDate, NaiveTime, TimeZone, Utc, Weekday};

use holidays::HOLIDAYS;

const RTH_OPEN: (u32, u32) = (9, 30);
const RTH_CLOSE: (u32, u32) = (16, 0);
const EOD_SWEEP: (u32, u32) = (16, 5);

fn et_offset() -> FixedOffset {
    FixedOffset::west_opt(5 * 3600).expect("ET offset is valid")
}

fn et_local_to_utc(date: NaiveDate, h: u32, m: u32) -> DateTime<Utc> {
    let naive = date.and_time(NaiveTime::from_hms_opt(h, m, 0).expect("valid time"));
    et_offset()
        .from_local_datetime(&naive)
        .single()
        .expect("ET is a fixed offset, no ambiguity")
        .with_timezone(&Utc)
}

fn is_weekend(date: NaiveDate) -> bool {
    matches!(date.weekday(), Weekday::Sat | Weekday::Sun)
}

fn is_business_day(date: NaiveDate) -> bool {
    !is_weekend(date) && !is_holiday(date)
}

/// True when `now` falls inside the US equity regular trading hours (09:30–16:00 ET)
/// on a non-weekend, non-holiday weekday.
pub fn is_rth_open(now: DateTime<Utc>) -> bool {
    let et = now.with_timezone(&et_offset());
    let date = et.date_naive();
    if !is_business_day(date) {
        return false;
    }
    let time = et.time();
    let open = NaiveTime::from_hms_opt(RTH_OPEN.0, RTH_OPEN.1, 0).expect("valid time");
    let close = NaiveTime::from_hms_opt(RTH_CLOSE.0, RTH_CLOSE.1, 0).expect("valid time");
    time >= open && time < close
}

/// True when `date` is a US equity market holiday (full-day close).
pub fn is_holiday(date: NaiveDate) -> bool {
    HOLIDAYS.binary_search(&date).is_ok()
}

/// Next 09:30 ET open time at or after `now`. Returns today's open when called
/// before today's open on a business day; otherwise advances to the next business day.
pub fn next_open_at(now: DateTime<Utc>) -> DateTime<Utc> {
    let et = now.with_timezone(&et_offset());
    let date = et.date_naive();
    let today_open = et_local_to_utc(date, RTH_OPEN.0, RTH_OPEN.1);
    if is_business_day(date) && now < today_open {
        return today_open;
    }
    let mut d = date;
    loop {
        d = d.succ_opt().expect("date arithmetic does not overflow");
        if is_business_day(d) {
            return et_local_to_utc(d, RTH_OPEN.0, RTH_OPEN.1);
        }
    }
}

/// Next 16:00 ET close time at or after `now`. Returns today's close when called
/// before today's close on a business day; otherwise advances to the next business day.
pub fn next_close_at(now: DateTime<Utc>) -> DateTime<Utc> {
    let et = now.with_timezone(&et_offset());
    let date = et.date_naive();
    let today_close = et_local_to_utc(date, RTH_CLOSE.0, RTH_CLOSE.1);
    if is_business_day(date) && now < today_close {
        return today_close;
    }
    let mut d = date;
    loop {
        d = d.succ_opt().expect("date arithmetic does not overflow");
        if is_business_day(d) {
            return et_local_to_utc(d, RTH_CLOSE.0, RTH_CLOSE.1);
        }
    }
}

/// 16:05 ET on the given date — the moment the EOD sweep should fire.
pub fn eod_sweep_target(date: NaiveDate) -> DateTime<Utc> {
    et_local_to_utc(date, EOD_SWEEP.0, EOD_SWEEP.1)
}

/// Walk forward `n` business days from `date` (skipping weekends + holidays)
/// and return the resulting `NaiveDate`. `n = 0` returns `date` unchanged.
pub fn trading_days_after(date: NaiveDate, n: u32) -> NaiveDate {
    let mut d = date;
    let mut remaining = n;
    while remaining > 0 {
        d = d.succ_opt().expect("date arithmetic does not overflow");
        if is_business_day(d) {
            remaining -= 1;
        }
    }
    d
}

/// Convenience wrapper used by the tracker state machine — anchored to
/// the *current* `now`'s ET date, advance `n` trading days, and return
/// the 16:00 ET RTH close as a UTC `DateTime`. This is what `in_play_until`
/// and `cool_down_until` get stamped to.
pub fn trading_days_after_close(now: DateTime<Utc>, n: u32) -> DateTime<Utc> {
    let today_et = now.with_timezone(&et_offset()).date_naive();
    let target = trading_days_after(today_et, n);
    et_local_to_utc(target, RTH_CLOSE.0, RTH_CLOSE.1)
}
