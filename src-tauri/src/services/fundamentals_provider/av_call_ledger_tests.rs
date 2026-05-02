//! Tests for [`AvCallLedger`]. Cover the per-symbol cap, daily soft/hard
//! caps, daily rollover, and SQLite-backed restart survival described
//! in `loop/plan/phase-5-cutover.md`.

use std::sync::{Arc, Mutex};

use chrono::NaiveDate;
use tempfile::NamedTempFile;

use super::av_call_ledger::{
    AvCallLedger, AvLedgerError, DateSource, ReserveOutcome, DEFAULT_HARD_CAP, DEFAULT_SOFT_CAP,
};
use crate::storage::Db;

/// Programmable date source so daily-rollover tests don't depend on
/// wall-clock time.
struct FixedDate(Mutex<NaiveDate>);

impl FixedDate {
    fn new(date: NaiveDate) -> Self {
        Self(Mutex::new(date))
    }

    fn set(&self, date: NaiveDate) {
        *self.0.lock().unwrap() = date;
    }
}

impl DateSource for FixedDate {
    fn today(&self) -> NaiveDate {
        *self.0.lock().unwrap()
    }
}

fn open_db() -> (NamedTempFile, Arc<Db>) {
    let tmp = NamedTempFile::new().unwrap();
    let db = Arc::new(Db::open(tmp.path()).unwrap());
    (tmp, db)
}

fn date(year: i32, month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, month, day).unwrap()
}

#[tokio::test]
async fn check_below_soft_cap_returns_below_and_does_not_increment() {
    let (_tmp, db) = open_db();
    let clock = Arc::new(FixedDate::new(date(2026, 5, 2)));
    let ledger = AvCallLedger::with_caps(db, DEFAULT_SOFT_CAP, DEFAULT_HARD_CAP, 5, clock);
    let outcome = ledger.check("AAPL").await.unwrap();
    assert_eq!(outcome, ReserveOutcome::BelowSoftCap);
    // Check alone must NOT increment — the AV call hasn't happened yet.
    assert_eq!(ledger.daily_count_today().await.unwrap(), 0);
    assert_eq!(ledger.per_symbol_count_today("AAPL").await.unwrap(), 0);

    ledger.commit("AAPL").await.unwrap();
    assert_eq!(ledger.daily_count_today().await.unwrap(), 1);
    assert_eq!(ledger.per_symbol_count_today("AAPL").await.unwrap(), 1);
}

#[tokio::test]
async fn per_symbol_cap_blocks_repeat_same_day() {
    let (_tmp, db) = open_db();
    let clock = Arc::new(FixedDate::new(date(2026, 5, 2)));
    let ledger = AvCallLedger::with_caps(db, 20, 25, 1, clock);
    ledger.check("AAPL").await.unwrap();
    ledger.commit("AAPL").await.unwrap();
    let err = ledger.check("AAPL").await.expect_err("must trip");
    match err {
        AvLedgerError::PerSymbolCapReached { symbol, count } => {
            assert_eq!(symbol, "AAPL");
            assert_eq!(count, 1);
        }
        other => panic!("expected PerSymbolCapReached, got {other:?}"),
    }
    // Different symbol still passes — protects same-day per-symbol cap
    // without locking the whole quota.
    let outcome = ledger.check("MSFT").await.unwrap();
    assert_eq!(outcome, ReserveOutcome::BelowSoftCap);
}

#[tokio::test]
async fn soft_cap_emits_above_soft_outcome() {
    let (_tmp, db) = open_db();
    let clock = Arc::new(FixedDate::new(date(2026, 5, 2)));
    // soft=2, hard=5, per-symbol=10 (irrelevant here).
    let ledger = AvCallLedger::with_caps(db, 2, 5, 10, clock);
    assert_eq!(
        ledger.check("AAPL").await.unwrap(),
        ReserveOutcome::BelowSoftCap
    );
    ledger.commit("AAPL").await.unwrap();
    assert_eq!(
        ledger.check("MSFT").await.unwrap(),
        ReserveOutcome::BelowSoftCap
    );
    ledger.commit("MSFT").await.unwrap();
    // 3rd consult crosses soft cap.
    assert_eq!(
        ledger.check("GOOG").await.unwrap(),
        ReserveOutcome::AboveSoftCap
    );
}

#[tokio::test]
async fn hard_cap_refuses_with_typed_error() {
    let (_tmp, db) = open_db();
    let clock = Arc::new(FixedDate::new(date(2026, 5, 2)));
    let ledger = AvCallLedger::with_caps(db, 2, 3, 10, clock);
    for sym in ["AAPL", "MSFT", "GOOG"] {
        ledger.check(sym).await.unwrap();
        ledger.commit(sym).await.unwrap();
    }
    let err = ledger.check("AMZN").await.expect_err("must trip");
    match err {
        AvLedgerError::DailyCapReached { hit_count } => {
            assert_eq!(hit_count, 3);
        }
        other => panic!("expected DailyCapReached, got {other:?}"),
    }
}

#[tokio::test]
async fn daily_rollover_resets_counts() {
    let (_tmp, db) = open_db();
    let clock = Arc::new(FixedDate::new(date(2026, 5, 2)));
    let ledger = AvCallLedger::with_caps(db, 2, 3, 1, Arc::clone(&clock) as Arc<dyn DateSource>);
    for sym in ["AAPL", "MSFT", "GOOG"] {
        ledger.check(sym).await.unwrap();
        ledger.commit(sym).await.unwrap();
    }
    // hard cap reached; AAPL is per-symbol-capped already.
    assert!(matches!(
        ledger.check("AAPL").await.expect_err("must trip"),
        AvLedgerError::PerSymbolCapReached { .. }
    ));

    // Roll the clock forward; counts reset for the new day.
    clock.set(date(2026, 5, 3));
    assert_eq!(ledger.daily_count_today().await.unwrap(), 0);
    let outcome = ledger.check("AAPL").await.unwrap();
    assert_eq!(outcome, ReserveOutcome::BelowSoftCap);
    ledger.commit("AAPL").await.unwrap();
    assert_eq!(ledger.daily_count_today().await.unwrap(), 1);
}

#[tokio::test]
async fn restart_survives_via_sqlite() {
    let tmp = NamedTempFile::new().unwrap();
    let path = tmp.path().to_path_buf();
    let clock = Arc::new(FixedDate::new(date(2026, 5, 2)));

    {
        let db = Arc::new(Db::open(&path).unwrap());
        let ledger =
            AvCallLedger::with_caps(db, 20, 25, 5, Arc::clone(&clock) as Arc<dyn DateSource>);
        for sym in ["AAPL", "MSFT", "GOOG"] {
            ledger.check(sym).await.unwrap();
            ledger.commit(sym).await.unwrap();
        }
        assert_eq!(ledger.daily_count_today().await.unwrap(), 3);
    }
    // Drop ledger + Db, simulate restart by re-opening the same file.
    {
        let db = Arc::new(Db::open(&path).unwrap());
        let ledger =
            AvCallLedger::with_caps(db, 20, 25, 5, Arc::clone(&clock) as Arc<dyn DateSource>);
        // Restart preserves per-day count and per-symbol counts.
        assert_eq!(ledger.daily_count_today().await.unwrap(), 3);
        assert_eq!(ledger.per_symbol_count_today("AAPL").await.unwrap(), 1);
        // Per-symbol cap still trips after restart at the configured cap.
        for _ in 0..4 {
            ledger.check("AAPL").await.unwrap();
            ledger.commit("AAPL").await.unwrap();
        }
        assert!(matches!(
            ledger.check("AAPL").await.expect_err("per-symbol trip"),
            AvLedgerError::PerSymbolCapReached { .. }
        ));
    }
}

#[tokio::test]
async fn case_insensitive_per_symbol_key() {
    let (_tmp, db) = open_db();
    let clock = Arc::new(FixedDate::new(date(2026, 5, 2)));
    let ledger = AvCallLedger::with_caps(db, 20, 25, 1, clock);
    ledger.check("aapl").await.unwrap();
    ledger.commit("aapl").await.unwrap();
    let err = ledger.check("AAPL").await.expect_err("must trip");
    assert!(matches!(err, AvLedgerError::PerSymbolCapReached { .. }));
}

#[tokio::test]
async fn failed_commit_does_not_burn_a_ticket_when_check_only() {
    // The increment happens only in commit(); a failed AV call between
    // check() and commit() leaves the ledger untouched. This is the
    // semantic that protects the daily quota from transport errors.
    let (_tmp, db) = open_db();
    let clock = Arc::new(FixedDate::new(date(2026, 5, 2)));
    let ledger = AvCallLedger::with_caps(db, 20, 25, 5, clock);
    ledger.check("AAPL").await.unwrap();
    // Caller decided AV failed; never calls commit.
    assert_eq!(ledger.daily_count_today().await.unwrap(), 0);
    assert_eq!(ledger.per_symbol_count_today("AAPL").await.unwrap(), 0);
    // Next check still passes.
    assert_eq!(
        ledger.check("AAPL").await.unwrap(),
        ReserveOutcome::BelowSoftCap
    );
}
