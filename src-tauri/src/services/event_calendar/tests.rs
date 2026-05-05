//! Composition tests for `EventCalendarService`. The earnings + FOMC
//! tier tests live alongside their respective modules; this file
//! exercises `is_blackout` in combinations that the runner will see.

use std::sync::Arc;

use chrono::{NaiveDate, TimeZone, Utc};
use tempfile::NamedTempFile;

use crate::storage::Db;

use super::earnings::{CompositeEarningsCalendar, EarningsCalendar, NoOpUpstream};
use super::earnings_store::{EarningsCacheStore, EarningsOverridesStore};
use super::fomc::FomcCalendar;
use super::types::{BlackoutConfidence, BlackoutKind, BlackoutPolicy, EarningsPolicy};
use super::EventCalendarService;

fn open_db() -> (NamedTempFile, Arc<Db>) {
    let tmp = NamedTempFile::new().unwrap();
    let db = Arc::new(Db::open(tmp.path()).unwrap());
    (tmp, db)
}

fn build_service(
    db: Arc<Db>,
    fomc_dates: Vec<NaiveDate>,
) -> (
    EventCalendarService,
    Arc<EarningsOverridesStore>,
    Arc<EarningsCacheStore>,
) {
    let overrides = Arc::new(EarningsOverridesStore::new(Arc::clone(&db)));
    let cache = Arc::new(EarningsCacheStore::new(Arc::clone(&db)));
    let upstream = Arc::new(NoOpUpstream);
    let composite: Arc<dyn EarningsCalendar> = Arc::new(CompositeEarningsCalendar::new(
        Arc::clone(&overrides),
        Arc::clone(&cache),
        upstream,
    ));
    let fomc = Arc::new(FomcCalendar::from_dates(fomc_dates));
    let svc = EventCalendarService::new(composite, fomc);
    (svc, overrides, cache)
}

fn permissive_policy() -> BlackoutPolicy {
    BlackoutPolicy {
        earnings: EarningsPolicy {
            bd_pre: 5,
            bd_post: 1,
            // Use false for tests so absent earnings means "not blacked out"
            // — easier to assert positive cases without seeding overrides.
            skip_if_unknown: false,
        },
        fomc: super::types::FomcPolicy { enabled: true },
    }
}

fn at_et_noon(d: NaiveDate) -> chrono::DateTime<Utc> {
    use crate::utils::market_calendar::et_offset;
    use chrono::{NaiveTime, TimeZone};
    let naive = d.and_time(NaiveTime::from_hms_opt(12, 0, 0).unwrap());
    et_offset()
        .from_local_datetime(&naive)
        .single()
        .unwrap()
        .with_timezone(&Utc)
}

#[tokio::test]
async fn earnings_4_bd_before_pivot_is_inside_5_bd_window() {
    let (_tmp, db) = open_db();
    let (svc, overrides, _cache) = build_service(db, Vec::new());

    let pivot = NaiveDate::from_ymd_opt(2026, 5, 14).unwrap(); // Thursday
    overrides
        .upsert("AAPL", pivot, BlackoutConfidence::Confirmed, "test", None)
        .await
        .unwrap();

    // 4 trading days before 2026-05-14: 2026-05-08 (Friday). Window
    // start is 2026-05-07 (5 BD before). 2026-05-08 should be inside.
    let at = at_et_noon(NaiveDate::from_ymd_opt(2026, 5, 8).unwrap());
    let policy = permissive_policy();
    let blackout = svc
        .is_blackout("AAPL", at, &policy)
        .await
        .unwrap()
        .expect("must be inside earnings window 4 BD before pivot");
    assert_eq!(blackout.kind, BlackoutKind::Earnings);
    assert_eq!(blackout.pivot_date, pivot);
    assert_eq!(blackout.source, "manual");
}

#[tokio::test]
async fn earnings_6_bd_before_pivot_is_outside_window() {
    let (_tmp, db) = open_db();
    let (svc, overrides, _cache) = build_service(db, Vec::new());

    let pivot = NaiveDate::from_ymd_opt(2026, 5, 14).unwrap();
    overrides
        .upsert("AAPL", pivot, BlackoutConfidence::Confirmed, "test", None)
        .await
        .unwrap();

    // 6 BD before pivot — outside the 5 BD window. 2026-05-06 is 6 BD
    // before 2026-05-14 (skipping no holidays).
    let at = at_et_noon(NaiveDate::from_ymd_opt(2026, 5, 6).unwrap());
    let policy = permissive_policy();
    assert!(svc
        .is_blackout("AAPL", at, &policy)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn earnings_window_respects_holidays() {
    // Memorial Day 2026 is Monday 2026-05-25. Pivot 2026-05-29 (Fri).
    // Walking back 5 BDs from 2026-05-29:
    //   05-28 Thu (1), 05-27 Wed (2), 05-26 Tue (3),
    //   skip 05-25 Memorial Day,
    //   05-22 Fri (4), 05-21 Thu (5).
    // So window-start = 2026-05-21. Without holiday-skip it would be
    // 2026-05-22 — one BD off. That's the divergence we test.
    let (_tmp, db) = open_db();
    let (svc, overrides, _cache) = build_service(db, Vec::new());

    let pivot = NaiveDate::from_ymd_opt(2026, 5, 29).unwrap();
    overrides
        .upsert("AAPL", pivot, BlackoutConfidence::Confirmed, "test", None)
        .await
        .unwrap();

    let policy = permissive_policy();
    let at_in = at_et_noon(NaiveDate::from_ymd_opt(2026, 5, 21).unwrap());
    assert!(
        svc.is_blackout("AAPL", at_in, &policy)
            .await
            .unwrap()
            .is_some(),
        "2026-05-21 is the 5-BD-before-pivot window-start (Memorial Day skipped)"
    );
    // One BD earlier (2026-05-20 Wed) is outside the window.
    let at_out = at_et_noon(NaiveDate::from_ymd_opt(2026, 5, 20).unwrap());
    assert!(
        svc.is_blackout("AAPL", at_out, &policy)
            .await
            .unwrap()
            .is_none(),
        "2026-05-20 is 6 BD before pivot — outside"
    );
}

#[tokio::test]
async fn earnings_post_window_includes_t_plus_1_bd() {
    let (_tmp, db) = open_db();
    let (svc, overrides, _cache) = build_service(db, Vec::new());

    let pivot = NaiveDate::from_ymd_opt(2026, 5, 14).unwrap(); // Thursday
    overrides
        .upsert("AAPL", pivot, BlackoutConfidence::Confirmed, "test", None)
        .await
        .unwrap();

    // 1 BD after pivot is 2026-05-15 (Friday). With bd_post = 1 the
    // window includes 2026-05-15 in full.
    let at = at_et_noon(NaiveDate::from_ymd_opt(2026, 5, 15).unwrap());
    let policy = permissive_policy();
    assert!(svc
        .is_blackout("AAPL", at, &policy)
        .await
        .unwrap()
        .is_some());

    // 2 BD after pivot is Monday 2026-05-18. With bd_post = 1 we are
    // outside the window.
    let at = at_et_noon(NaiveDate::from_ymd_opt(2026, 5, 18).unwrap());
    assert!(svc
        .is_blackout("AAPL", at, &policy)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn fomc_window_returns_blackout_at_14_30_et() {
    let (_tmp, db) = open_db();
    let (svc, _o, _c) = build_service(db, vec![NaiveDate::from_ymd_opt(2026, 5, 6).unwrap()]);

    let at = at_et_noon(NaiveDate::from_ymd_opt(2026, 5, 6).unwrap()); // 12:00 ET — outside FOMC
    let policy = permissive_policy();
    assert!(
        svc.is_blackout("AAPL", at, &policy)
            .await
            .unwrap()
            .is_none(),
        "noon ET on FOMC day is outside the 14:00-16:00 ET window"
    );

    use crate::utils::market_calendar::et_offset;
    use chrono::{NaiveTime, TimeZone};
    let naive = NaiveDate::from_ymd_opt(2026, 5, 6)
        .unwrap()
        .and_time(NaiveTime::from_hms_opt(14, 30, 0).unwrap());
    let at = et_offset()
        .from_local_datetime(&naive)
        .single()
        .unwrap()
        .with_timezone(&Utc);
    let blackout = svc
        .is_blackout("AAPL", at, &policy)
        .await
        .unwrap()
        .expect("FOMC blackout must hit");
    assert_eq!(blackout.kind, BlackoutKind::Fomc);
}

#[tokio::test]
async fn skip_if_unknown_synth_blackout() {
    let (_tmp, db) = open_db();
    let (svc, _o, _c) = build_service(db, Vec::new());

    let policy = BlackoutPolicy {
        earnings: EarningsPolicy {
            bd_pre: 5,
            bd_post: 1,
            skip_if_unknown: true,
        },
        ..Default::default()
    };
    let at = Utc.with_ymd_and_hms(2026, 5, 6, 14, 0, 0).unwrap();
    let blackout = svc
        .is_blackout("ZZZZ", at, &policy)
        .await
        .unwrap()
        .expect("skip_if_unknown=true must surface a synth blackout");
    assert_eq!(blackout.source, "unknown");
    assert_eq!(blackout.kind, BlackoutKind::Earnings);
}

#[tokio::test]
async fn disabled_earnings_policy_short_circuits() {
    // Episodic-pivot's policy: bd_pre = bd_post = 0. The gate must
    // not query the earnings calendar at all (and so unknown-symbol
    // can't synth a blackout).
    let (_tmp, db) = open_db();
    let (svc, _o, _c) = build_service(db, Vec::new());

    let policy = BlackoutPolicy {
        earnings: EarningsPolicy {
            bd_pre: 0,
            bd_post: 0,
            skip_if_unknown: true, // ignored when disabled
        },
        ..Default::default()
    };
    let at = Utc.with_ymd_and_hms(2026, 5, 6, 14, 0, 0).unwrap();
    assert!(
        svc.is_blackout("ZZZZ", at, &policy)
            .await
            .unwrap()
            .is_none(),
        "earnings disabled → no earnings blackout, even with skip_if_unknown"
    );
}

#[tokio::test]
async fn fomc_disabled_policy_skips_market_wide_blackout() {
    let (_tmp, db) = open_db();
    let (svc, _o, _c) = build_service(db, vec![NaiveDate::from_ymd_opt(2026, 5, 6).unwrap()]);

    let policy = BlackoutPolicy {
        earnings: EarningsPolicy {
            bd_pre: 0,
            bd_post: 0,
            skip_if_unknown: false,
        },
        fomc: super::types::FomcPolicy { enabled: false },
    };

    use crate::utils::market_calendar::et_offset;
    use chrono::{NaiveTime, TimeZone};
    let naive = NaiveDate::from_ymd_opt(2026, 5, 6)
        .unwrap()
        .and_time(NaiveTime::from_hms_opt(14, 30, 0).unwrap());
    let at = et_offset()
        .from_local_datetime(&naive)
        .single()
        .unwrap()
        .with_timezone(&Utc);
    assert!(svc
        .is_blackout("AAPL", at, &policy)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn lookup_returns_trading_days_until_earnings() {
    let (_tmp, db) = open_db();
    let (svc, overrides, _cache) = build_service(db, Vec::new());

    let pivot = NaiveDate::from_ymd_opt(2026, 5, 14).unwrap(); // Thursday
    overrides
        .upsert("AAPL", pivot, BlackoutConfidence::Confirmed, "test", None)
        .await
        .unwrap();

    // From 2026-05-06 (Wednesday) to 2026-05-14 (Thursday): 6 BDs.
    let at = at_et_noon(NaiveDate::from_ymd_opt(2026, 5, 6).unwrap());
    let lookup = svc.lookup("AAPL", at).await.unwrap();
    let earnings = lookup.next_earnings.unwrap();
    assert_eq!(earnings.date, pivot);
    assert_eq!(earnings.trading_days_until, 6);
}
