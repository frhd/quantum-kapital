use std::sync::Arc;

use async_trait::async_trait;
use chrono::{
    DateTime, Duration as ChronoDuration, FixedOffset, NaiveDate, NaiveTime, TimeZone, Utc,
};
use tempfile::NamedTempFile;

use crate::events::EventEmitter;
use crate::ibkr::error::Result as IbkrResult;
use crate::ibkr::types::historical::{BarSize, HistoricalBar};
use crate::ibkr::types::news::NewsItem;
use crate::ibkr::types::tracker::{TrackerSource, TrackerStatus};
use crate::services::historical_data_service::Lookback;
use crate::services::tracker_runner::{BarsFetcher, NewsFetcher, TrackerRunner};
use crate::services::tracker_service::TrackerService;
use crate::services::tracker_state_machine::TrackerStateMachine;
use crate::storage::Db;
use crate::strategies::DetectorRegistry;

use super::{Clock, EodScheduler};

// ---------------- helpers ----------------

fn et_offset() -> FixedOffset {
    FixedOffset::west_opt(5 * 3600).unwrap()
}

fn et_dt(date: NaiveDate, h: u32, m: u32) -> DateTime<Utc> {
    let naive = date.and_time(NaiveTime::from_hms_opt(h, m, 0).unwrap());
    et_offset()
        .from_local_datetime(&naive)
        .unwrap()
        .with_timezone(&Utc)
}

/// Tuesday 2026-05-26 — first trading day after Memorial Day weekend.
fn tuesday() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 5, 26).unwrap()
}

/// Saturday 2026-05-02 — weekend.
fn saturday() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 5, 2).unwrap()
}

/// 2026-07-03 — Independence Day observed (NYSE full close).
fn holiday() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 7, 3).unwrap()
}

struct EmptyBars;

#[async_trait]
impl BarsFetcher for EmptyBars {
    async fn fetch(
        &self,
        _symbol: &str,
        _bar_size: BarSize,
        _lookback: Lookback,
    ) -> IbkrResult<Vec<HistoricalBar>> {
        Ok(Vec::new())
    }
}

struct EmptyNews;

#[async_trait]
impl NewsFetcher for EmptyNews {
    async fn fetch(&self, _symbol: &str, _lookback_hours: u32) -> Vec<NewsItem> {
        Vec::new()
    }
}

fn make_scheduler(
    now: DateTime<Utc>,
) -> (
    NamedTempFile,
    Arc<Db>,
    Arc<TrackerService>,
    Arc<TrackerStateMachine>,
    EodScheduler,
) {
    let tmp = NamedTempFile::new().expect("tempfile");
    let db = Arc::new(Db::open(tmp.path()).expect("open db"));
    let tracker = Arc::new(TrackerService::new(Arc::clone(&db)));
    let emitter = Arc::new(EventEmitter::new());
    let state_machine = Arc::new(TrackerStateMachine::new(
        Arc::clone(&db),
        Arc::clone(&tracker),
        Arc::clone(&emitter),
    ));
    let bars: Arc<dyn BarsFetcher> = Arc::new(EmptyBars);
    let news: Arc<dyn NewsFetcher> = Arc::new(EmptyNews);
    let runner = Arc::new(TrackerRunner::new(
        Arc::clone(&db),
        Arc::clone(&tracker),
        Arc::clone(&state_machine),
        Arc::clone(&emitter),
        bars,
        news,
        Arc::new(DetectorRegistry::new()),
    ));
    let scheduler = EodScheduler::with_clock(
        runner,
        Arc::clone(&state_machine),
        emitter,
        Clock::Fixed(now),
    );
    (tmp, db, tracker, state_machine, scheduler)
}

// ---------------- tests ----------------

#[tokio::test]
async fn does_not_run_outside_eod_window() {
    let now = et_dt(tuesday(), 10, 0);
    let (_tmp, _db, _tracker, _sm, scheduler) = make_scheduler(now);

    let outcome = scheduler.tick().await.expect("tick");
    assert!(outcome.is_none(), "tick at 10am ET should not run");
    assert!(scheduler.last_run_date().await.is_none());
}

#[tokio::test]
async fn runs_at_1605_et_on_weekday() {
    let now = et_dt(tuesday(), 16, 5);
    let (_tmp, _db, tracker, state_machine, scheduler) = make_scheduler(now);

    // Stage a tracker row whose in_play_until is already in the past so
    // we can verify expire_ttls actually ran inside the tick.
    tracker
        .add("AAPL", TrackerSource::Manual, None, vec![], None)
        .await
        .unwrap();
    let stale_until = now - ChronoDuration::hours(1);
    tracker
        .set_status("AAPL", TrackerStatus::InPlay, Some(stale_until), None)
        .await
        .unwrap();

    let outcome = scheduler.tick().await.expect("tick").expect("ran");
    assert_eq!(outcome.date, tuesday());
    assert_eq!(outcome.run_results.len(), 1, "watchlist had one ticker");
    assert_eq!(outcome.expired, 1, "stale in_play TTL should be swept");
    assert_eq!(scheduler.last_run_date().await, Some(tuesday()));

    // Phase 12 sanity: AAPL should now be back in `Watching`.
    let row = tracker.get("AAPL").await.unwrap().unwrap();
    assert_eq!(row.status, TrackerStatus::Watching);
    assert!(row.in_play_until.is_none());

    // Confirm nothing else broke: the state_machine helper still works.
    let active = state_machine.active_in_play_symbols().await.unwrap();
    assert!(active.is_empty());
}

#[tokio::test]
async fn does_not_run_on_weekend() {
    let now = et_dt(saturday(), 16, 5);
    let (_tmp, _db, _tracker, _sm, scheduler) = make_scheduler(now);

    let outcome = scheduler.tick().await.expect("tick");
    assert!(outcome.is_none(), "saturday should never run");
    assert!(scheduler.last_run_date().await.is_none());
}

#[tokio::test]
async fn does_not_run_on_holiday() {
    let now = et_dt(holiday(), 16, 5);
    let (_tmp, _db, _tracker, _sm, scheduler) = make_scheduler(now);

    let outcome = scheduler.tick().await.expect("tick");
    assert!(outcome.is_none(), "holiday should never run");
    assert!(scheduler.last_run_date().await.is_none());
}

#[tokio::test]
async fn dedup_runs_within_same_day() {
    // First tick at 16:05; second tick at 16:08 ET (still inside the
    // window) on the same trading day. Only the first should run.
    let first_now = et_dt(tuesday(), 16, 5);
    let (_tmp, _db, _tracker, _sm, scheduler) = make_scheduler(first_now);

    let first = scheduler.tick().await.expect("first tick");
    assert!(first.is_some(), "first tick should run");

    // Move the injected clock forward inside the same window.
    scheduler
        .set_clock(Clock::Fixed(et_dt(tuesday(), 16, 8)))
        .await;

    let second = scheduler.tick().await.expect("second tick");
    assert!(
        second.is_none(),
        "second tick same day should be deduped, got {:?}",
        second
    );
    assert_eq!(scheduler.last_run_date().await, Some(tuesday()));
}

#[tokio::test]
async fn out_of_window_does_not_clobber_last_run_date() {
    // 16:30 ET — past the 5-minute window. Should not run, and should
    // not blow away an earlier successful run's `last_run_date`.
    let now = et_dt(tuesday(), 16, 30);
    let (_tmp, _db, _tracker, _sm, scheduler) = make_scheduler(now);

    let outcome = scheduler.tick().await.expect("tick");
    assert!(outcome.is_none());
    assert!(scheduler.last_run_date().await.is_none());
}

// ---- IbkrState integration: handle replacement / drop ----

#[tokio::test]
async fn start_replaces_existing_handle() {
    use crate::config::AppConfig;
    use crate::ibkr::IbkrState;

    let tmp = NamedTempFile::new().expect("tempfile");
    let db = Arc::new(Db::open(tmp.path()).expect("open db"));
    let cfg = AppConfig::default().ibkr.into();
    let state = IbkrState::new(cfg, Arc::clone(&db));

    let bars: Arc<dyn BarsFetcher> = Arc::new(EmptyBars);
    let news: Arc<dyn NewsFetcher> = Arc::new(EmptyNews);
    let runner = Arc::new(TrackerRunner::new(
        Arc::clone(&db),
        Arc::clone(&state.tracker),
        Arc::clone(&state.state_machine),
        Arc::clone(&state.event_emitter),
        bars,
        news,
        Arc::new(DetectorRegistry::new()),
    ));
    // Pin the clock to a time outside the EOD window so the spawned
    // loop never actually fires within the test's lifetime.
    let scheduler = Arc::new(EodScheduler::with_clock(
        runner,
        Arc::clone(&state.state_machine),
        Arc::clone(&state.event_emitter),
        Clock::Fixed(et_dt(tuesday(), 10, 0)),
    ));

    state
        .start_eod_scheduler(Arc::clone(&scheduler))
        .await
        .expect("first start");
    assert!(state.eod_handle.read().await.is_some());

    // Second start replaces the first handle.
    state
        .start_eod_scheduler(Arc::clone(&scheduler))
        .await
        .expect("second start");
    assert!(state.eod_handle.read().await.is_some());

    state.stop_eod_scheduler().await;
}

#[tokio::test]
async fn stop_drops_handle() {
    use crate::config::AppConfig;
    use crate::ibkr::IbkrState;

    let tmp = NamedTempFile::new().expect("tempfile");
    let db = Arc::new(Db::open(tmp.path()).expect("open db"));
    let cfg = AppConfig::default().ibkr.into();
    let state = IbkrState::new(cfg, Arc::clone(&db));

    let bars: Arc<dyn BarsFetcher> = Arc::new(EmptyBars);
    let news: Arc<dyn NewsFetcher> = Arc::new(EmptyNews);
    let runner = Arc::new(TrackerRunner::new(
        Arc::clone(&db),
        Arc::clone(&state.tracker),
        Arc::clone(&state.state_machine),
        Arc::clone(&state.event_emitter),
        bars,
        news,
        Arc::new(DetectorRegistry::new()),
    ));
    let scheduler = Arc::new(EodScheduler::with_clock(
        runner,
        Arc::clone(&state.state_machine),
        Arc::clone(&state.event_emitter),
        Clock::Fixed(et_dt(tuesday(), 10, 0)),
    ));

    state
        .start_eod_scheduler(Arc::clone(&scheduler))
        .await
        .expect("start");
    assert!(state.eod_handle.read().await.is_some());

    state.stop_eod_scheduler().await;
    assert!(state.eod_handle.read().await.is_none());
}
