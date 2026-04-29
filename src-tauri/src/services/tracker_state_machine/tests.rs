// allow-large-file: state-machine transition matrix (watching → in_play → cool_down)
// needs many fixture scenarios to cover every edge; one shared test DB harness
// powers them all.
use std::sync::Arc;

use chrono::{Duration as ChronoDuration, FixedOffset, NaiveDate, NaiveTime, TimeZone, Utc};
use serde_json::json;
use tempfile::NamedTempFile;

use crate::events::{AppEvent, EventEmitter};
use crate::ibkr::types::tracker::{SetupStatus, StrategyTag, TrackerSource, TrackerStatus};
use crate::ibkr::types::BarSize;
use crate::services::tracker_service::TrackerService;
use crate::storage::Db;
use crate::strategies::{Direction, SetupCandidate, TargetLevel};
use crate::utils::market_calendar::trading_days_after_close;

use super::{
    Clock, StateMachineError, TrackerStateMachine, COOL_DOWN_TRADING_DAYS, IN_PLAY_TRADING_DAYS,
};

fn et_offset() -> FixedOffset {
    FixedOffset::west_opt(5 * 3600).unwrap()
}

fn et_dt(date: NaiveDate, h: u32, m: u32) -> chrono::DateTime<Utc> {
    let naive = date.and_time(NaiveTime::from_hms_opt(h, m, 0).unwrap());
    et_offset()
        .from_local_datetime(&naive)
        .unwrap()
        .with_timezone(&Utc)
}

fn fri_2026_05_01_10am_et() -> chrono::DateTime<Utc> {
    et_dt(NaiveDate::from_ymd_opt(2026, 5, 1).unwrap(), 10, 0)
}

fn make_fixtures(
    now: chrono::DateTime<Utc>,
) -> (
    NamedTempFile,
    Arc<TrackerService>,
    TrackerStateMachine,
    Arc<EventEmitter>,
) {
    let tmp = NamedTempFile::new().expect("tempfile");
    let db = Arc::new(Db::open(tmp.path()).expect("open db"));
    let tracker = Arc::new(TrackerService::new(Arc::clone(&db)));
    let emitter = Arc::new(EventEmitter::for_capture());
    let sm = TrackerStateMachine::with_clock(
        Arc::clone(&db),
        Arc::clone(&tracker),
        Arc::clone(&emitter),
        Clock::Fixed(now),
    );
    (tmp, tracker, sm, emitter)
}

fn sample_candidate(direction: Direction) -> SetupCandidate {
    SetupCandidate {
        strategy: "breakout",
        tag: StrategyTag::Breakout,
        direction,
        conviction_signal: 0.7,
        trigger_price: 105.0,
        stop_price: 100.0,
        targets: vec![
            TargetLevel {
                label: "2R".to_string(),
                price: 115.0,
            },
            TargetLevel {
                label: "3R".to_string(),
                price: 120.0,
            },
        ],
        raw_signals: json!({"volume_multiple": 1.8}),
        timeframe: BarSize::Day1,
        detected_at: Utc::now(),
    }
}

#[tokio::test]
async fn watching_promoted_to_in_play_on_scanner_add() {
    let now = fri_2026_05_01_10am_et();
    let (_tmp, tracker, sm, _emitter) = make_fixtures(now);
    tracker
        .add("AAPL", TrackerSource::Scanner, None, vec![], None)
        .await
        .unwrap();

    sm.record_scanner_hit("AAPL", Some(json!({"rank": 1})))
        .await
        .unwrap();

    let row = tracker.get("AAPL").await.unwrap().unwrap();
    assert_eq!(row.status, TrackerStatus::InPlay);
    let expected = trading_days_after_close(now, IN_PLAY_TRADING_DAYS);
    assert_eq!(row.in_play_until.unwrap().timestamp(), expected.timestamp());
    assert_eq!(row.cool_down_until, None);
    assert_eq!(row.source_meta, Some(json!({"rank": 1})));
}

#[tokio::test]
async fn watching_promoted_to_setup_active_on_detector_hit() {
    let now = fri_2026_05_01_10am_et();
    let (_tmp, tracker, sm, _emitter) = make_fixtures(now);
    tracker
        .add("AAPL", TrackerSource::Manual, None, vec![], None)
        .await
        .unwrap();
    let setup = tracker
        .insert_setup("AAPL", &sample_candidate(Direction::Long))
        .await
        .unwrap();

    sm.on_setup_detected("AAPL", setup.id).await.unwrap();

    let row = tracker.get("AAPL").await.unwrap().unwrap();
    assert_eq!(row.status, TrackerStatus::SetupActive);
    let expected = trading_days_after_close(now, IN_PLAY_TRADING_DAYS);
    assert_eq!(row.in_play_until.unwrap().timestamp(), expected.timestamp());
}

#[tokio::test]
async fn in_play_promoted_to_setup_active_on_detector_hit() {
    let now = fri_2026_05_01_10am_et();
    let (_tmp, tracker, sm, _emitter) = make_fixtures(now);
    tracker
        .add("AAPL", TrackerSource::Scanner, None, vec![], None)
        .await
        .unwrap();
    sm.record_scanner_hit("AAPL", None).await.unwrap();
    let setup = tracker
        .insert_setup("AAPL", &sample_candidate(Direction::Long))
        .await
        .unwrap();

    sm.on_setup_detected("AAPL", setup.id).await.unwrap();

    let row = tracker.get("AAPL").await.unwrap().unwrap();
    assert_eq!(row.status, TrackerStatus::SetupActive);
    assert!(row.in_play_until.is_some());
}

#[tokio::test]
async fn setup_active_to_cool_down_on_invalidate() {
    let now = fri_2026_05_01_10am_et();
    let (_tmp, tracker, sm, _emitter) = make_fixtures(now);
    tracker
        .add("AAPL", TrackerSource::Manual, None, vec![], None)
        .await
        .unwrap();
    let setup = tracker
        .insert_setup("AAPL", &sample_candidate(Direction::Long))
        .await
        .unwrap();
    sm.on_setup_detected("AAPL", setup.id).await.unwrap();

    sm.mark_invalidated(setup.id, "stop hit").await.unwrap();

    let row = tracker.get("AAPL").await.unwrap().unwrap();
    assert_eq!(row.status, TrackerStatus::CoolDown);
    let expected = trading_days_after_close(now, COOL_DOWN_TRADING_DAYS);
    assert_eq!(
        row.cool_down_until.unwrap().timestamp(),
        expected.timestamp()
    );
    assert_eq!(row.in_play_until, None);

    let s = tracker.get_setup(setup.id).await.unwrap().unwrap();
    assert_eq!(s.status, SetupStatus::Invalidated);
    assert_eq!(s.invalidation_reason.as_deref(), Some("stop hit"));
    assert!(s.invalidated_at.is_some());
}

#[tokio::test]
async fn setup_active_to_cool_down_on_target_hit() {
    let now = fri_2026_05_01_10am_et();
    let (_tmp, tracker, sm, _emitter) = make_fixtures(now);
    tracker
        .add("AAPL", TrackerSource::Manual, None, vec![], None)
        .await
        .unwrap();
    let setup = tracker
        .insert_setup("AAPL", &sample_candidate(Direction::Long))
        .await
        .unwrap();
    sm.on_setup_detected("AAPL", setup.id).await.unwrap();

    sm.mark_completed(setup.id).await.unwrap();

    let row = tracker.get("AAPL").await.unwrap().unwrap();
    assert_eq!(row.status, TrackerStatus::CoolDown);
    assert!(row.cool_down_until.is_some());

    let s = tracker.get_setup(setup.id).await.unwrap().unwrap();
    assert_eq!(s.status, SetupStatus::Completed);
}

#[tokio::test]
async fn cool_down_to_watching_on_ttl_expiry() {
    // Pin the clock so the cool-down stamp is deterministic, then call
    // `expire_ttls` with a `now` past the cool_down_until.
    let now = fri_2026_05_01_10am_et();
    let (_tmp, tracker, sm, _emitter) = make_fixtures(now);
    tracker
        .add("AAPL", TrackerSource::Manual, None, vec![], None)
        .await
        .unwrap();
    let setup = tracker
        .insert_setup("AAPL", &sample_candidate(Direction::Long))
        .await
        .unwrap();
    sm.on_setup_detected("AAPL", setup.id).await.unwrap();
    sm.mark_invalidated(setup.id, "stop hit").await.unwrap();
    let row = tracker.get("AAPL").await.unwrap().unwrap();
    let cool_down_until = row.cool_down_until.unwrap();

    // One second past the cool-down stamp.
    let after = cool_down_until + ChronoDuration::seconds(1);
    let n = sm.expire_ttls(after).await.unwrap();
    assert_eq!(n, 1);

    let row = tracker.get("AAPL").await.unwrap().unwrap();
    assert_eq!(row.status, TrackerStatus::Watching);
    assert_eq!(row.cool_down_until, None);
    assert_eq!(row.in_play_until, None);
}

#[tokio::test]
async fn in_play_to_watching_on_ttl_expiry() {
    let now = fri_2026_05_01_10am_et();
    let (_tmp, tracker, sm, _emitter) = make_fixtures(now);
    tracker
        .add("AAPL", TrackerSource::Scanner, None, vec![], None)
        .await
        .unwrap();
    sm.record_scanner_hit("AAPL", None).await.unwrap();
    let row = tracker.get("AAPL").await.unwrap().unwrap();
    let in_play_until = row.in_play_until.unwrap();

    let after = in_play_until + ChronoDuration::seconds(1);
    let n = sm.expire_ttls(after).await.unwrap();
    assert_eq!(n, 1);

    let row = tracker.get("AAPL").await.unwrap().unwrap();
    assert_eq!(row.status, TrackerStatus::Watching);
    assert_eq!(row.in_play_until, None);
}

#[tokio::test]
async fn expire_ttls_is_idempotent() {
    let now = fri_2026_05_01_10am_et();
    let (_tmp, tracker, sm, _emitter) = make_fixtures(now);
    tracker
        .add("AAPL", TrackerSource::Scanner, None, vec![], None)
        .await
        .unwrap();
    sm.record_scanner_hit("AAPL", None).await.unwrap();
    let row = tracker.get("AAPL").await.unwrap().unwrap();
    let in_play_until = row.in_play_until.unwrap();

    let after = in_play_until + ChronoDuration::seconds(1);
    let first = sm.expire_ttls(after).await.unwrap();
    let second = sm.expire_ttls(after).await.unwrap();
    assert_eq!(first, 1);
    assert_eq!(second, 0);
}

#[tokio::test]
async fn expire_ttls_uses_trading_days_not_calendar_days() {
    // Anchor at Fri 2026-05-01 10:00 ET. record_scanner_hit stamps
    // in_play_until = 16:00 ET on Wed 2026-05-06 (Fri+3 trading days).
    // 5 calendar days after Friday = Wed 2026-05-06 — which IS the same
    // wall date here, so we can't distinguish purely on the count.
    // Instead: verify that 4 calendar days after Friday (Tue 2026-05-05
    // 23:00 UTC, well past midnight) does NOT trip the expiry, because
    // by trading-day math we still owe Wed.
    let fri = NaiveDate::from_ymd_opt(2026, 5, 1).unwrap();
    let now = et_dt(fri, 10, 0);
    let (_tmp, tracker, sm, _emitter) = make_fixtures(now);
    tracker
        .add("AAPL", TrackerSource::Scanner, None, vec![], None)
        .await
        .unwrap();
    sm.record_scanner_hit("AAPL", None).await.unwrap();

    let row = tracker.get("AAPL").await.unwrap().unwrap();
    let in_play_until = row.in_play_until.unwrap();

    // Expected: Wed 2026-05-06 16:00 ET = 21:00 UTC.
    let expected = et_dt(NaiveDate::from_ymd_opt(2026, 5, 6).unwrap(), 16, 0);
    assert_eq!(in_play_until.timestamp(), expected.timestamp());

    // Tue 2026-05-05 23:59 UTC — past midnight on the 5th trading day
    // boundary if you interpreted "3 trading days" as calendar days from
    // Mon — should NOT yet expire.
    let tue_2359_utc = chrono::Utc.with_ymd_and_hms(2026, 5, 5, 23, 59, 0).unwrap();
    assert!(tue_2359_utc < expected);
    let n = sm.expire_ttls(tue_2359_utc).await.unwrap();
    assert_eq!(n, 0);

    // Wed 2026-05-06 16:00 ET → expire fires.
    let n = sm.expire_ttls(expected).await.unwrap();
    assert_eq!(n, 1);
}

#[tokio::test]
async fn multiple_active_setups_only_last_invalidation_flips_status() {
    let now = fri_2026_05_01_10am_et();
    let (_tmp, tracker, sm, _emitter) = make_fixtures(now);
    tracker
        .add("AAPL", TrackerSource::Manual, None, vec![], None)
        .await
        .unwrap();
    let setup_long = tracker
        .insert_setup("AAPL", &sample_candidate(Direction::Long))
        .await
        .unwrap();
    let setup_short = tracker
        .insert_setup("AAPL", &sample_candidate(Direction::Short))
        .await
        .unwrap();
    sm.on_setup_detected("AAPL", setup_long.id).await.unwrap();

    // Invalidate one — ticker stays SetupActive (other still active).
    sm.mark_invalidated(setup_long.id, "stop hit")
        .await
        .unwrap();
    let row = tracker.get("AAPL").await.unwrap().unwrap();
    assert_eq!(row.status, TrackerStatus::SetupActive);
    assert_eq!(row.cool_down_until, None);

    // Invalidate the second — ticker flips to CoolDown.
    sm.mark_invalidated(setup_short.id, "thesis broken")
        .await
        .unwrap();
    let row = tracker.get("AAPL").await.unwrap().unwrap();
    assert_eq!(row.status, TrackerStatus::CoolDown);
    assert!(row.cool_down_until.is_some());
}

#[tokio::test]
async fn mark_invalidated_returns_error_for_unknown_setup() {
    let now = fri_2026_05_01_10am_et();
    let (_tmp, _tracker, sm, _emitter) = make_fixtures(now);
    let err = sm
        .mark_invalidated(9999, "no such setup")
        .await
        .expect_err("must err");
    match err {
        StateMachineError::SetupNotFound(id) => assert_eq!(id, 9999),
        other => panic!("expected SetupNotFound, got {other:?}"),
    }
}

#[tokio::test]
async fn active_in_play_symbols_returns_in_play_and_setup_active() {
    let now = fri_2026_05_01_10am_et();
    let (_tmp, tracker, sm, _emitter) = make_fixtures(now);
    tracker
        .add("AAPL", TrackerSource::Manual, None, vec![], None)
        .await
        .unwrap();
    tracker
        .add("MSFT", TrackerSource::Scanner, None, vec![], None)
        .await
        .unwrap();
    tracker
        .add("NVDA", TrackerSource::Manual, None, vec![], None)
        .await
        .unwrap();

    // AAPL stays Watching, MSFT goes InPlay, NVDA goes SetupActive.
    sm.record_scanner_hit("MSFT", None).await.unwrap();
    let setup = tracker
        .insert_setup("NVDA", &sample_candidate(Direction::Long))
        .await
        .unwrap();
    sm.on_setup_detected("NVDA", setup.id).await.unwrap();

    let mut got = sm.active_in_play_symbols().await.unwrap();
    got.sort();
    assert_eq!(got, vec!["MSFT".to_string(), "NVDA".to_string()]);
}

#[tokio::test]
async fn record_scanner_hit_on_unknown_symbol_is_noop() {
    let now = fri_2026_05_01_10am_et();
    let (_tmp, tracker, sm, _emitter) = make_fixtures(now);
    sm.record_scanner_hit("NOSUCH", None).await.unwrap();
    assert!(tracker.get("NOSUCH").await.unwrap().is_none());
}

// ---------------- Phase 15: event emission ----------------

#[tokio::test]
async fn ticker_status_changed_event_on_promotion() {
    let now = fri_2026_05_01_10am_et();
    let (_tmp, tracker, sm, emitter) = make_fixtures(now);
    tracker
        .add("AAPL", TrackerSource::Scanner, None, vec![], None)
        .await
        .unwrap();

    sm.record_scanner_hit("AAPL", None).await.unwrap();

    let events = emitter.captured().await;
    let status_events: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            AppEvent::TickerStatusChanged { symbol, from, to } => {
                Some((symbol.clone(), *from, *to))
            }
            _ => None,
        })
        .collect();
    assert_eq!(status_events.len(), 1);
    let (sym, from, to) = &status_events[0];
    assert_eq!(sym, "AAPL");
    assert_eq!(*from, TrackerStatus::Watching);
    assert_eq!(*to, TrackerStatus::InPlay);
}

#[tokio::test]
async fn setup_invalidated_event_emitted_on_state_machine_transition() {
    let now = fri_2026_05_01_10am_et();
    let (_tmp, tracker, sm, emitter) = make_fixtures(now);
    tracker
        .add("AAPL", TrackerSource::Manual, None, vec![], None)
        .await
        .unwrap();
    let setup = tracker
        .insert_setup("AAPL", &sample_candidate(Direction::Long))
        .await
        .unwrap();
    sm.on_setup_detected("AAPL", setup.id).await.unwrap();

    sm.mark_invalidated(setup.id, "stop hit").await.unwrap();

    let events = emitter.captured().await;
    let invalidations: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            AppEvent::SetupInvalidated {
                setup_id,
                symbol,
                reason,
            } => Some((*setup_id, symbol.clone(), reason.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(invalidations.len(), 1);
    assert_eq!(invalidations[0].0, setup.id);
    assert_eq!(invalidations[0].1, "AAPL");
    assert_eq!(invalidations[0].2, "stop hit");
}

#[tokio::test]
async fn ticker_status_changed_event_on_invalidation_cool_down() {
    let now = fri_2026_05_01_10am_et();
    let (_tmp, tracker, sm, emitter) = make_fixtures(now);
    tracker
        .add("AAPL", TrackerSource::Manual, None, vec![], None)
        .await
        .unwrap();
    let setup = tracker
        .insert_setup("AAPL", &sample_candidate(Direction::Long))
        .await
        .unwrap();
    sm.on_setup_detected("AAPL", setup.id).await.unwrap();
    sm.mark_invalidated(setup.id, "stop hit").await.unwrap();

    let events = emitter.captured().await;
    // Two TickerStatusChanged events: Watching→SetupActive then
    // SetupActive→CoolDown (only one active setup so cool-down fires).
    let transitions: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            AppEvent::TickerStatusChanged { from, to, .. } => Some((*from, *to)),
            _ => None,
        })
        .collect();
    assert_eq!(
        transitions,
        vec![
            (TrackerStatus::Watching, TrackerStatus::SetupActive),
            (TrackerStatus::SetupActive, TrackerStatus::CoolDown),
        ]
    );
}

#[tokio::test]
async fn expire_ttls_emits_status_changed_per_flipped_row() {
    let now = fri_2026_05_01_10am_et();
    let (_tmp, tracker, sm, emitter) = make_fixtures(now);
    tracker
        .add("AAPL", TrackerSource::Scanner, None, vec![], None)
        .await
        .unwrap();
    sm.record_scanner_hit("AAPL", None).await.unwrap();
    let row = tracker.get("AAPL").await.unwrap().unwrap();
    let in_play_until = row.in_play_until.unwrap();

    // Drain capture so only the post-expire events remain.
    let _ = emitter.captured().await;

    let after = in_play_until + ChronoDuration::seconds(1);
    sm.expire_ttls(after).await.unwrap();

    let events = emitter.captured().await;
    let post: Vec<_> = events
        .iter()
        .rev()
        .find_map(|e| match e {
            AppEvent::TickerStatusChanged { symbol, from, to } => {
                Some((symbol.clone(), *from, *to))
            }
            _ => None,
        })
        .into_iter()
        .collect();
    assert_eq!(post.len(), 1);
    assert_eq!(post[0].0, "AAPL");
    assert_eq!(post[0].1, TrackerStatus::InPlay);
    assert_eq!(post[0].2, TrackerStatus::Watching);
}

// ---------------- Phase 21: alert recording ----------------

#[tokio::test]
async fn alert_inserted_on_setup_invalidated() {
    use crate::ibkr::types::tracker::AlertKind;
    use crate::services::alerts::{list_alerts, ListAlertsQuery};

    let now = fri_2026_05_01_10am_et();
    let (_tmp, tracker, sm, _emitter) = make_fixtures(now);
    tracker
        .add("AAPL", TrackerSource::Manual, None, vec![], None)
        .await
        .unwrap();
    let setup = tracker
        .insert_setup("AAPL", &sample_candidate(Direction::Long))
        .await
        .unwrap();
    sm.on_setup_detected("AAPL", setup.id).await.unwrap();

    sm.mark_invalidated(setup.id, "stop hit").await.unwrap();

    // We get the Db through the tracker service's pool; the state
    // machine and the tracker service share the same `Arc<Db>` via
    // `make_fixtures`, so list_alerts on a fresh handle reads the
    // same store.
    let db = tracker_db_from(&tracker);
    let alerts = list_alerts(&db, ListAlertsQuery::default())
        .await
        .expect("list");
    let invalidated: Vec<_> = alerts
        .iter()
        .filter(|a| a.kind == AlertKind::Invalidated)
        .collect();
    assert_eq!(invalidated.len(), 1);
    assert_eq!(invalidated[0].setup_id, setup.id);
    assert_eq!(invalidated[0].payload["symbol"], "AAPL");
    assert_eq!(invalidated[0].payload["reason"], "stop hit");
    assert!(!invalidated[0].seen);
}

#[tokio::test]
async fn alert_inserted_on_setup_target_hit() {
    use crate::ibkr::types::tracker::AlertKind;
    use crate::services::alerts::{list_alerts, ListAlertsQuery};

    let now = fri_2026_05_01_10am_et();
    let (_tmp, tracker, sm, _emitter) = make_fixtures(now);
    tracker
        .add("AAPL", TrackerSource::Manual, None, vec![], None)
        .await
        .unwrap();
    let setup = tracker
        .insert_setup("AAPL", &sample_candidate(Direction::Long))
        .await
        .unwrap();
    sm.on_setup_detected("AAPL", setup.id).await.unwrap();

    sm.mark_completed(setup.id).await.unwrap();

    let db = tracker_db_from(&tracker);
    let alerts = list_alerts(&db, ListAlertsQuery::default())
        .await
        .expect("list");
    let target_hits: Vec<_> = alerts
        .iter()
        .filter(|a| a.kind == AlertKind::TargetHit)
        .collect();
    assert_eq!(target_hits.len(), 1);
    assert_eq!(target_hits[0].setup_id, setup.id);
    assert_eq!(target_hits[0].payload["symbol"], "AAPL");
}

/// Test helper: pull the shared `Arc<Db>` back out by opening a new
/// `Db` against the same temp path.  Cheap because the underlying file
/// is the same and r2d2 hands us a fresh connection.  Used by the
/// Phase 21 alert-wiring tests.
fn tracker_db_from(_tracker: &Arc<TrackerService>) -> Arc<crate::storage::Db> {
    // The tracker service holds a private Arc<Db>; re-use it through a
    // minimal accessor so the alert-wiring tests don't need to be
    // refactored to thread the Db through `make_fixtures`. Keeping the
    // helper local keeps the test surface honest about the dependency.
    _tracker.db_for_testing()
}

#[tokio::test]
async fn ticker_status_changed_serializes_with_snake_case_status() {
    // The frontend types map snake-case status strings; verify the
    // wire payload matches.
    let event = AppEvent::TickerStatusChanged {
        symbol: "AAPL".to_string(),
        from: TrackerStatus::Watching,
        to: TrackerStatus::InPlay,
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "TickerStatusChanged");
    assert_eq!(json["data"]["symbol"], "AAPL");
    assert_eq!(json["data"]["from"], "watching");
    assert_eq!(json["data"]["to"], "in_play");
}
