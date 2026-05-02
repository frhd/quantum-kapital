use std::sync::Arc;

use chrono::{Duration as ChronoDuration, Utc};
use serde_json::json;
use tempfile::NamedTempFile;

use crate::ibkr::types::tracker::{AlertKind, TrackerSource};
use crate::services::tracker_service::TrackerService;
use crate::storage::Db;
use crate::strategies::{Direction, SetupCandidate, TargetLevel};

use super::{list_alerts, mark_alerts_seen, record_alert, ListAlertsQuery};

fn make_db() -> (NamedTempFile, Arc<Db>) {
    let tmp = NamedTempFile::new().expect("tempfile");
    let db = Arc::new(Db::open(tmp.path()).expect("open db"));
    (tmp, db)
}

fn sample_candidate(direction: Direction) -> SetupCandidate {
    SetupCandidate {
        strategy: "breakout",
        tag: crate::ibkr::types::tracker::StrategyTag::Breakout,
        direction,
        conviction_signal: 0.7,
        trigger_price: 105.0,
        stop_price: 100.0,
        targets: vec![TargetLevel {
            label: "2R".to_string(),
            price: 115.0,
        }],
        raw_signals: json!({"volume_multiple": 1.8}),
        timeframe: crate::ibkr::types::historical::BarSize::Day1,
        detected_at: Utc::now(),
    }
}

async fn seed_setup(db: &Arc<Db>, symbol: &str) -> i64 {
    let svc = TrackerService::new(Arc::clone(db));
    svc.add(symbol, TrackerSource::Manual, None, vec![], None)
        .await
        .expect("add ticker");
    svc.insert_setup(symbol, &sample_candidate(Direction::Long))
        .await
        .expect("insert setup")
        .id
}

#[tokio::test]
async fn record_alert_inserts_row_and_returns_unseen() {
    let (_tmp, db) = make_db();
    let setup_id = seed_setup(&db, "AAPL").await;

    let alert = record_alert(
        &db,
        setup_id,
        AlertKind::Detected,
        json!({"symbol": "AAPL", "trigger_price": 105.0}),
    )
    .await
    .expect("record")
    .expect("inserted");

    assert_eq!(alert.setup_id, setup_id);
    assert_eq!(alert.kind, AlertKind::Detected);
    assert!(!alert.seen);
    assert_eq!(alert.payload["symbol"], "AAPL");

    let listed = list_alerts(&db, ListAlertsQuery::default())
        .await
        .expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, alert.id);
}

#[tokio::test]
async fn alerts_dedup_per_event_within_window() {
    let (_tmp, db) = make_db();
    let setup_id = seed_setup(&db, "AAPL").await;

    let first = record_alert(
        &db,
        setup_id,
        AlertKind::Detected,
        json!({"symbol": "AAPL"}),
    )
    .await
    .expect("first");
    assert!(first.is_some(), "first record must persist");

    let second = record_alert(
        &db,
        setup_id,
        AlertKind::Detected,
        json!({"symbol": "AAPL", "now": "again"}),
    )
    .await
    .expect("second");
    assert!(
        second.is_none(),
        "second record within DEDUP_WINDOW must be suppressed"
    );

    let listed = list_alerts(&db, ListAlertsQuery::default())
        .await
        .expect("list");
    assert_eq!(listed.len(), 1, "only one row persisted");
}

#[tokio::test]
async fn dedup_does_not_collapse_distinct_kinds() {
    let (_tmp, db) = make_db();
    let setup_id = seed_setup(&db, "AAPL").await;

    record_alert(&db, setup_id, AlertKind::Detected, json!({}))
        .await
        .unwrap()
        .expect("inserted detected");
    record_alert(&db, setup_id, AlertKind::Invalidated, json!({}))
        .await
        .unwrap()
        .expect("inserted invalidated");
    record_alert(&db, setup_id, AlertKind::ThesisChanged, json!({}))
        .await
        .unwrap()
        .expect("inserted thesis_changed");

    let listed = list_alerts(&db, ListAlertsQuery::default())
        .await
        .expect("list");
    assert_eq!(listed.len(), 3);
}

#[tokio::test]
async fn list_alerts_pagination_and_filters() {
    let (_tmp, db) = make_db();
    let setup_a = seed_setup(&db, "AAPL").await;
    let setup_b = seed_setup(&db, "MSFT").await;

    // Mix of kinds across two setups, spaced over time so the dedup
    // window doesn't trip. We back-date by writing directly via a
    // helper that bypasses dedup — but for the test we just rely on
    // distinct (setup_id, kind) pairs.
    record_alert(
        &db,
        setup_a,
        AlertKind::Detected,
        json!({"symbol": "AAPL", "n": 1}),
    )
    .await
    .unwrap();
    record_alert(
        &db,
        setup_a,
        AlertKind::Invalidated,
        json!({"symbol": "AAPL", "n": 2}),
    )
    .await
    .unwrap();
    record_alert(
        &db,
        setup_b,
        AlertKind::Detected,
        json!({"symbol": "MSFT", "n": 3}),
    )
    .await
    .unwrap();
    record_alert(
        &db,
        setup_b,
        AlertKind::ThesisChanged,
        json!({"symbol": "MSFT", "n": 4}),
    )
    .await
    .unwrap();

    // Default: newest first.
    let listed = list_alerts(&db, ListAlertsQuery::default())
        .await
        .expect("list");
    assert_eq!(listed.len(), 4);
    assert_eq!(listed[0].payload["n"], 4);
    assert_eq!(listed.last().unwrap().payload["n"], 1);

    // Limit 2 + offset 1 → middle of the list.
    let page = list_alerts(
        &db,
        ListAlertsQuery {
            limit: 2,
            offset: 1,
            ..Default::default()
        },
    )
    .await
    .expect("page");
    assert_eq!(page.len(), 2);
    assert_eq!(page[0].payload["n"], 3);
    assert_eq!(page[1].payload["n"], 2);

    // Filter by kind.
    let detected_only = list_alerts(
        &db,
        ListAlertsQuery {
            kind: Some(AlertKind::Detected),
            ..Default::default()
        },
    )
    .await
    .expect("kind filter");
    assert_eq!(detected_only.len(), 2);
    for a in &detected_only {
        assert_eq!(a.kind, AlertKind::Detected);
    }

    // Filter by since (after the last write — should return nothing).
    let future = Utc::now() + ChronoDuration::seconds(10);
    let nothing = list_alerts(
        &db,
        ListAlertsQuery {
            since: Some(future),
            ..Default::default()
        },
    )
    .await
    .expect("since filter");
    assert!(nothing.is_empty());
}

#[tokio::test]
async fn mark_alerts_seen_flips_only_listed_ids() {
    let (_tmp, db) = make_db();
    let setup_id = seed_setup(&db, "AAPL").await;

    let a = record_alert(&db, setup_id, AlertKind::Detected, json!({}))
        .await
        .unwrap()
        .unwrap();
    let b = record_alert(&db, setup_id, AlertKind::Invalidated, json!({}))
        .await
        .unwrap()
        .unwrap();
    let c = record_alert(&db, setup_id, AlertKind::ThesisChanged, json!({}))
        .await
        .unwrap()
        .unwrap();

    let n = mark_alerts_seen(&db, vec![a.id, c.id])
        .await
        .expect("mark seen");
    assert_eq!(n, 2);

    let unseen = list_alerts(
        &db,
        ListAlertsQuery {
            only_unseen: true,
            ..Default::default()
        },
    )
    .await
    .expect("only_unseen");
    assert_eq!(unseen.len(), 1);
    assert_eq!(unseen[0].id, b.id);

    // Idempotent — re-marking already-seen ids reports 0 rows flipped.
    let again = mark_alerts_seen(&db, vec![a.id, c.id])
        .await
        .expect("mark seen again");
    assert_eq!(again, 0);
}

#[tokio::test]
async fn mark_alerts_seen_empty_input_is_noop() {
    let (_tmp, db) = make_db();
    let n = mark_alerts_seen(&db, vec![]).await.expect("empty");
    assert_eq!(n, 0);
}

#[tokio::test]
async fn list_alerts_filters_by_symbol_via_setups_join() {
    let (_tmp, db) = make_db();
    let setup_a = seed_setup(&db, "AAPL").await;
    let setup_b = seed_setup(&db, "MSFT").await;

    record_alert(
        &db,
        setup_a,
        AlertKind::Detected,
        json!({"symbol": "AAPL", "n": 1}),
    )
    .await
    .unwrap()
    .expect("aapl detected");
    record_alert(
        &db,
        setup_a,
        AlertKind::Invalidated,
        json!({"symbol": "AAPL", "n": 2}),
    )
    .await
    .unwrap()
    .expect("aapl invalidated");
    record_alert(
        &db,
        setup_b,
        AlertKind::Detected,
        json!({"symbol": "MSFT", "n": 3}),
    )
    .await
    .unwrap()
    .expect("msft detected");

    let aapl_only = list_alerts(
        &db,
        ListAlertsQuery {
            symbol: Some("AAPL".to_string()),
            ..Default::default()
        },
    )
    .await
    .expect("symbol filter");
    assert_eq!(aapl_only.len(), 2);
    assert!(aapl_only.iter().all(|a| a.setup_id == setup_a));

    // Lower-case input still matches (uppercased before query).
    let aapl_lower = list_alerts(
        &db,
        ListAlertsQuery {
            symbol: Some("aapl".to_string()),
            ..Default::default()
        },
    )
    .await
    .expect("case insensitive symbol");
    assert_eq!(aapl_lower.len(), 2);

    // Symbol filter composes with kind filter.
    let aapl_detected = list_alerts(
        &db,
        ListAlertsQuery {
            symbol: Some("AAPL".to_string()),
            kind: Some(AlertKind::Detected),
            ..Default::default()
        },
    )
    .await
    .expect("symbol+kind");
    assert_eq!(aapl_detected.len(), 1);
    assert_eq!(aapl_detected[0].kind, AlertKind::Detected);

    // Unknown symbol → empty.
    let none = list_alerts(
        &db,
        ListAlertsQuery {
            symbol: Some("NVDA".to_string()),
            ..Default::default()
        },
    )
    .await
    .expect("unknown symbol");
    assert!(none.is_empty());
}

#[tokio::test]
async fn list_alerts_unenriched_only_filters_marked_rows() {
    use super::mark_alert_enriched;

    let (_tmp, db) = make_db();
    let setup_id = seed_setup(&db, "AAPL").await;

    let a = record_alert(&db, setup_id, AlertKind::Detected, json!({"n": 1}))
        .await
        .unwrap()
        .unwrap();
    let b = record_alert(&db, setup_id, AlertKind::Invalidated, json!({"n": 2}))
        .await
        .unwrap()
        .unwrap();
    let _c = record_alert(&db, setup_id, AlertKind::ThesisChanged, json!({"n": 3}))
        .await
        .unwrap()
        .unwrap();

    // Mark `a` as enriched (skipped — no note id).
    mark_alert_enriched(&db, a.id, None).await.unwrap();
    // Default listing still shows all rows.
    let all = list_alerts(&db, ListAlertsQuery::default())
        .await
        .expect("list");
    assert_eq!(all.len(), 3);
    // unenriched_only filters out `a`.
    let pending = list_alerts(
        &db,
        ListAlertsQuery {
            unenriched_only: true,
            ..Default::default()
        },
    )
    .await
    .expect("pending");
    assert_eq!(pending.len(), 2);
    let ids: Vec<i64> = pending.iter().map(|x| x.id).collect();
    assert!(!ids.contains(&a.id));
    assert!(ids.contains(&b.id));
}
