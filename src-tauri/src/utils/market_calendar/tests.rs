use super::*;
use chrono::{FixedOffset, NaiveDate, NaiveTime, TimeZone, Utc};

fn et() -> FixedOffset {
    FixedOffset::west_opt(5 * 3600).unwrap()
}

fn et_dt(date: NaiveDate, h: u32, m: u32) -> chrono::DateTime<Utc> {
    let naive = date.and_time(NaiveTime::from_hms_opt(h, m, 0).unwrap());
    et().from_local_datetime(&naive)
        .unwrap()
        .with_timezone(&Utc)
}

const TUE_2026_04_28: NaiveDate = match NaiveDate::from_ymd_opt(2026, 4, 28) {
    Some(d) => d,
    None => panic!("invalid date"),
};
const MON_2026_04_27: NaiveDate = match NaiveDate::from_ymd_opt(2026, 4, 27) {
    Some(d) => d,
    None => panic!("invalid date"),
};
const SAT_2026_05_02: NaiveDate = match NaiveDate::from_ymd_opt(2026, 5, 2) {
    Some(d) => d,
    None => panic!("invalid date"),
};
const SUN_2026_05_03: NaiveDate = match NaiveDate::from_ymd_opt(2026, 5, 3) {
    Some(d) => d,
    None => panic!("invalid date"),
};
const FRI_2026_05_01: NaiveDate = match NaiveDate::from_ymd_opt(2026, 5, 1) {
    Some(d) => d,
    None => panic!("invalid date"),
};
const FRI_2026_07_03_OBSERVED: NaiveDate = match NaiveDate::from_ymd_opt(2026, 7, 3) {
    Some(d) => d,
    None => panic!("invalid date"),
};
const THU_2026_07_02: NaiveDate = match NaiveDate::from_ymd_opt(2026, 7, 2) {
    Some(d) => d,
    None => panic!("invalid date"),
};
const MON_2026_07_06: NaiveDate = match NaiveDate::from_ymd_opt(2026, 7, 6) {
    Some(d) => d,
    None => panic!("invalid date"),
};
const MON_2026_05_04: NaiveDate = match NaiveDate::from_ymd_opt(2026, 5, 4) {
    Some(d) => d,
    None => panic!("invalid date"),
};

#[test]
fn is_rth_true_at_1000_et_on_weekday() {
    assert!(is_rth_open(et_dt(TUE_2026_04_28, 10, 0)));
}

#[test]
fn is_rth_false_at_0900_et() {
    assert!(!is_rth_open(et_dt(TUE_2026_04_28, 9, 0)));
}

#[test]
fn is_rth_false_at_1601_et() {
    assert!(!is_rth_open(et_dt(TUE_2026_04_28, 16, 1)));
}

#[test]
fn is_rth_false_on_saturday() {
    assert!(!is_rth_open(et_dt(SAT_2026_05_02, 10, 0)));
    assert!(!is_rth_open(et_dt(SAT_2026_05_02, 14, 0)));
}

#[test]
fn is_rth_false_on_sunday() {
    assert!(!is_rth_open(et_dt(SUN_2026_05_03, 10, 0)));
    assert!(!is_rth_open(et_dt(SUN_2026_05_03, 14, 0)));
}

#[test]
fn is_rth_false_on_holiday() {
    // 2026-07-03 is the observed Independence Day (July 4 falls on Saturday).
    assert!(!is_rth_open(et_dt(FRI_2026_07_03_OBSERVED, 10, 0)));
    assert!(is_holiday(FRI_2026_07_03_OBSERVED));
}

#[test]
fn next_open_at_returns_today_open_when_called_pre_open() {
    let now = et_dt(MON_2026_04_27, 8, 0);
    let expected = et_dt(MON_2026_04_27, 9, 30);
    assert_eq!(next_open_at(now), expected);
}

#[test]
fn next_open_at_returns_next_business_day_when_called_post_close() {
    let now = et_dt(MON_2026_04_27, 17, 0);
    let expected = et_dt(TUE_2026_04_28, 9, 30);
    assert_eq!(next_open_at(now), expected);
}

#[test]
fn next_open_at_skips_weekend() {
    let now = et_dt(FRI_2026_05_01, 17, 0);
    let expected = et_dt(MON_2026_05_04, 9, 30);
    assert_eq!(next_open_at(now), expected);
}

#[test]
fn next_open_at_skips_holiday() {
    // Day before observed July 4 (2026-07-03) at 17:00 ET → following business day's open.
    // 2026-07-02 (Thu) 17:00 ET → 2026-07-06 (Mon) 09:30 ET (skipping Fri holiday + weekend).
    let now = et_dt(THU_2026_07_02, 17, 0);
    let expected = et_dt(MON_2026_07_06, 9, 30);
    assert_eq!(next_open_at(now), expected);
}

#[test]
fn next_close_at_within_session() {
    let now = et_dt(TUE_2026_04_28, 14, 0);
    let expected = et_dt(TUE_2026_04_28, 16, 0);
    assert_eq!(next_close_at(now), expected);
}

#[test]
fn eod_sweep_target_is_1605_et() {
    let target = eod_sweep_target(TUE_2026_04_28);
    let expected = et_dt(TUE_2026_04_28, 16, 5);
    assert_eq!(target, expected);
}

#[test]
fn trading_days_after_zero_is_identity() {
    assert_eq!(trading_days_after(MON_2026_04_27, 0), MON_2026_04_27);
}

#[test]
fn trading_days_after_one_business_day() {
    // Mon → Tue
    assert_eq!(trading_days_after(MON_2026_04_27, 1), TUE_2026_04_28);
}

#[test]
fn trading_days_after_skips_weekend() {
    // Fri 2026-05-01 + 1 trading day → Mon 2026-05-04
    assert_eq!(trading_days_after(FRI_2026_05_01, 1), MON_2026_05_04);
}

#[test]
fn trading_days_after_three_from_friday_lands_on_wednesday() {
    // Fri 2026-05-01 + 3 trading days → Wed 2026-05-06 (Mon, Tue, Wed).
    let expected = NaiveDate::from_ymd_opt(2026, 5, 6).unwrap();
    assert_eq!(trading_days_after(FRI_2026_05_01, 3), expected);
}

#[test]
fn trading_days_after_skips_holiday() {
    // Thu 2026-07-02 + 1 trading day skips Fri 2026-07-03 (Independence Day
    // observed) and the weekend → Mon 2026-07-06.
    assert_eq!(trading_days_after(THU_2026_07_02, 1), MON_2026_07_06);
}

#[test]
fn trading_days_after_close_returns_1600_et_on_target_date() {
    // Anchor at Fri 2026-05-01 10:00 ET; +3 trading days → Wed 2026-05-06
    // 16:00 ET as UTC.
    let now = et_dt(FRI_2026_05_01, 10, 0);
    let expected = et_dt(NaiveDate::from_ymd_opt(2026, 5, 6).unwrap(), 16, 0);
    assert_eq!(trading_days_after_close(now, 3), expected);
}
