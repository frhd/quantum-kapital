use std::sync::Arc;

use chrono::{Duration as ChronoDuration, Utc};
use serde_json::json;
use tempfile::NamedTempFile;

use crate::ibkr::types::tracker::{StrategyTag, TrackerSource, TrackerStatus};
use crate::storage::Db;

use super::{TrackerError, TrackerService};

fn make_service() -> (NamedTempFile, TrackerService) {
    let tmp = NamedTempFile::new().expect("tempfile");
    let db = Db::open(tmp.path()).expect("open db");
    (tmp, TrackerService::new(Arc::new(db)))
}

/// Regression: catches an accidental `pub` → `pub(crate)` slip on the
/// `TrackerService` API after the Phase 25 split into `mod.rs` + `setups.rs`.
/// Imports the type by its public path and exercises both a ticker-CRUD
/// method (`add`) and a setup-CRUD method (`count_active_setups`) so the
/// `pub use setups::*;` (or equivalent) wiring is verified end-to-end.
#[tokio::test]
async fn tracker_service_split_compiles() {
    use crate::services::tracker_service::TrackerService as PublicTrackerService;
    let tmp = NamedTempFile::new().expect("tempfile");
    let db = Arc::new(Db::open(tmp.path()).expect("open db"));
    let svc = PublicTrackerService::new(db);
    svc.add("ABC", TrackerSource::Manual, None, vec![], None)
        .await
        .expect("add");
    let count = svc
        .count_active_setups("ABC")
        .await
        .expect("count_active_setups");
    assert_eq!(count, 0);
}

#[tokio::test]
async fn add_inserts_row_and_returns_typed_value() {
    let (_tmp, svc) = make_service();
    let row = svc
        .add(
            "AAPL",
            TrackerSource::Manual,
            Some(json!({"reason": "earnings"})),
            vec![StrategyTag::Breakout],
            Some("hot".to_string()),
        )
        .await
        .expect("add");
    assert_eq!(row.symbol, "AAPL");
    assert_eq!(row.source, TrackerSource::Manual);
    assert_eq!(row.status, TrackerStatus::Watching);
    assert_eq!(row.tags, vec![StrategyTag::Breakout]);
    assert_eq!(row.notes.as_deref(), Some("hot"));
    assert!(row.last_checked_at.is_none());
    assert!(row.in_play_until.is_none());

    // And it persists to disk — fetch back.
    let fetched = svc.get("AAPL").await.expect("get").expect("present");
    assert_eq!(fetched.symbol, "AAPL");
    assert_eq!(fetched.source_meta, Some(json!({"reason": "earnings"})));
}

#[tokio::test]
async fn add_normalizes_symbol_case() {
    let (_tmp, svc) = make_service();
    svc.add("tsla", TrackerSource::Scanner, None, vec![], None)
        .await
        .unwrap();
    let fetched = svc.get("TSLA").await.unwrap().unwrap();
    assert_eq!(fetched.symbol, "TSLA");
}

#[tokio::test]
async fn add_duplicate_symbol_errors() {
    let (_tmp, svc) = make_service();
    svc.add("AAPL", TrackerSource::Manual, None, vec![], None)
        .await
        .unwrap();
    let err = svc
        .add("AAPL", TrackerSource::Scanner, None, vec![], None)
        .await
        .expect_err("must error");
    match err {
        TrackerError::AlreadyTracked(s) => assert_eq!(s, "AAPL"),
        other => panic!("expected AlreadyTracked, got {other:?}"),
    }
}

#[tokio::test]
async fn remove_deletes_row() {
    let (_tmp, svc) = make_service();
    svc.add("AAPL", TrackerSource::Manual, None, vec![], None)
        .await
        .unwrap();
    svc.remove("AAPL").await.unwrap();
    assert!(svc.list(None).await.unwrap().is_empty());
}

#[tokio::test]
async fn remove_non_existent_is_idempotent() {
    let (_tmp, svc) = make_service();
    svc.remove("NOSUCH").await.expect("idempotent ok");
}

#[tokio::test]
async fn list_filters_by_status() {
    let (_tmp, svc) = make_service();
    svc.add("AAPL", TrackerSource::Manual, None, vec![], None)
        .await
        .unwrap();
    svc.add("MSFT", TrackerSource::Manual, None, vec![], None)
        .await
        .unwrap();
    svc.add("NVDA", TrackerSource::Manual, None, vec![], None)
        .await
        .unwrap();

    svc.set_status("MSFT", TrackerStatus::InPlay, None, None)
        .await
        .unwrap();
    svc.set_status("NVDA", TrackerStatus::SetupActive, None, None)
        .await
        .unwrap();

    let in_play = svc.list(Some(TrackerStatus::InPlay)).await.unwrap();
    assert_eq!(in_play.len(), 1);
    assert_eq!(in_play[0].symbol, "MSFT");

    let watching = svc.list(Some(TrackerStatus::Watching)).await.unwrap();
    assert_eq!(watching.len(), 1);
    assert_eq!(watching[0].symbol, "AAPL");

    let all = svc.list(None).await.unwrap();
    assert_eq!(all.len(), 3);
}

#[tokio::test]
async fn set_tags_replaces_tag_array() {
    let (_tmp, svc) = make_service();
    svc.add(
        "AAPL",
        TrackerSource::Manual,
        None,
        vec![StrategyTag::Breakout],
        None,
    )
    .await
    .unwrap();

    let updated = svc
        .set_tags(
            "AAPL",
            vec![StrategyTag::EpisodicPivot, StrategyTag::ParabolicShort],
        )
        .await
        .unwrap();
    assert_eq!(
        updated.tags,
        vec![StrategyTag::EpisodicPivot, StrategyTag::ParabolicShort]
    );

    // Round-trip after read-back.
    let fetched = svc.get("AAPL").await.unwrap().unwrap();
    assert_eq!(
        fetched.tags,
        vec![StrategyTag::EpisodicPivot, StrategyTag::ParabolicShort]
    );
}

#[tokio::test]
async fn set_tags_on_missing_returns_not_found() {
    let (_tmp, svc) = make_service();
    let err = svc.set_tags("NOSUCH", vec![]).await.expect_err("must err");
    match err {
        TrackerError::NotFound(s) => assert_eq!(s, "NOSUCH"),
        other => panic!("expected NotFound, got {other:?}"),
    }
}

#[tokio::test]
async fn set_status_updates_status_and_in_play_until() {
    let (_tmp, svc) = make_service();
    svc.add("AAPL", TrackerSource::Manual, None, vec![], None)
        .await
        .unwrap();

    let until = Utc::now() + ChronoDuration::days(3);
    let updated = svc
        .set_status("AAPL", TrackerStatus::InPlay, Some(until), None)
        .await
        .unwrap();
    assert_eq!(updated.status, TrackerStatus::InPlay);
    let stored_until = updated.in_play_until.expect("set");
    // Second-precision storage; allow off-by-one.
    assert!((stored_until.timestamp() - until.timestamp()).abs() <= 1);
}

#[tokio::test]
async fn tags_round_trip_via_json_with_custom_variant() {
    let (_tmp, svc) = make_service();
    svc.add(
        "AAPL",
        TrackerSource::Manual,
        None,
        vec![
            StrategyTag::Breakout,
            StrategyTag::Custom("squeeze".to_string()),
        ],
        None,
    )
    .await
    .unwrap();
    let fetched = svc.get("AAPL").await.unwrap().unwrap();
    assert_eq!(
        fetched.tags,
        vec![
            StrategyTag::Breakout,
            StrategyTag::Custom("squeeze".to_string()),
        ]
    );
}

#[tokio::test]
async fn source_meta_round_trip() {
    let (_tmp, svc) = make_service();
    let payload = json!({
        "scanner_code": "TOP_PERC_GAIN",
        "rank": 7,
        "details": {
            "exchange": "NASDAQ",
            "tags": ["leader", "high_rvol"]
        }
    });
    svc.add(
        "AAPL",
        TrackerSource::Scanner,
        Some(payload.clone()),
        vec![],
        None,
    )
    .await
    .unwrap();
    let fetched = svc.get("AAPL").await.unwrap().unwrap();
    assert_eq!(fetched.source_meta, Some(payload));
}

#[tokio::test]
async fn touch_last_checked_updates_field() {
    let (_tmp, svc) = make_service();
    svc.add("AAPL", TrackerSource::Manual, None, vec![], None)
        .await
        .unwrap();
    assert!(svc
        .get("AAPL")
        .await
        .unwrap()
        .unwrap()
        .last_checked_at
        .is_none());

    svc.touch_last_checked("AAPL").await.unwrap();
    let fetched = svc.get("AAPL").await.unwrap().unwrap();
    assert!(fetched.last_checked_at.is_some());
}

#[tokio::test]
async fn get_returns_none_for_missing() {
    let (_tmp, svc) = make_service();
    assert!(svc.get("NOSUCH").await.unwrap().is_none());
}

// ---------------- setup CRUD (Phase 10) ----------------

use crate::ibkr::types::tracker::SetupStatus;
use crate::ibkr::types::BarSize;
use crate::strategies::{Direction, SetupCandidate, TargetLevel};

fn sample_candidate() -> SetupCandidate {
    SetupCandidate {
        strategy: "breakout",
        tag: StrategyTag::Breakout,
        direction: Direction::Long,
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
        raw_signals: serde_json::json!({"volume_multiple": 1.8, "rsi_14": 65.0}),
        timeframe: BarSize::Day1,
        detected_at: Utc::now(),
    }
}

#[tokio::test]
async fn insert_setup_persists_row_and_returns_typed_value() {
    let (_tmp, svc) = make_service();
    svc.add("AAPL", TrackerSource::Manual, None, vec![], None)
        .await
        .unwrap();

    let cand = sample_candidate();
    let row = svc.insert_setup("AAPL", &cand).await.expect("insert");
    assert!(row.id > 0);
    assert_eq!(row.symbol, "AAPL");
    assert_eq!(row.strategy, "breakout");
    assert_eq!(row.direction, Direction::Long);
    assert_eq!(row.trigger_price, 105.0);
    assert_eq!(row.stop_price, 100.0);
    assert_eq!(row.targets.len(), 2);
    assert_eq!(row.status, SetupStatus::Active);
    assert!(row.thesis.is_none());

    let fetched = svc
        .get_setup(row.id)
        .await
        .expect("get_setup")
        .expect("present");
    assert_eq!(fetched.symbol, "AAPL");
    assert_eq!(fetched.raw_signals, cand.raw_signals);
    assert_eq!(fetched.targets, cand.targets);
}

#[tokio::test]
async fn insert_setup_normalizes_symbol_case() {
    let (_tmp, svc) = make_service();
    svc.add("tsla", TrackerSource::Manual, None, vec![], None)
        .await
        .unwrap();
    let row = svc.insert_setup("tsla", &sample_candidate()).await.unwrap();
    assert_eq!(row.symbol, "TSLA");
}

#[tokio::test]
async fn list_setups_filters_by_symbol_and_since() {
    let (_tmp, svc) = make_service();
    svc.add("AAPL", TrackerSource::Manual, None, vec![], None)
        .await
        .unwrap();
    svc.add("MSFT", TrackerSource::Manual, None, vec![], None)
        .await
        .unwrap();

    let now = Utc::now();
    let mut a = sample_candidate();
    a.detected_at = now - ChronoDuration::days(2);
    svc.insert_setup("AAPL", &a).await.unwrap();

    let mut b = sample_candidate();
    b.detected_at = now;
    svc.insert_setup("MSFT", &b).await.unwrap();

    // No filter — both rows.
    let all = svc.list_setups(None, None).await.unwrap();
    assert_eq!(all.len(), 2);
    // Most-recent first by detected_at DESC.
    assert_eq!(all[0].symbol, "MSFT");
    assert_eq!(all[1].symbol, "AAPL");

    // Symbol filter.
    let aapl_only = svc.list_setups(Some("AAPL"), None).await.unwrap();
    assert_eq!(aapl_only.len(), 1);
    assert_eq!(aapl_only[0].symbol, "AAPL");

    // Since filter excludes the older row.
    let recent = svc
        .list_setups(None, Some(now - ChronoDuration::hours(1)))
        .await
        .unwrap();
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].symbol, "MSFT");

    // Combined filters.
    let combo = svc
        .list_setups(Some("AAPL"), Some(now - ChronoDuration::hours(1)))
        .await
        .unwrap();
    assert!(combo.is_empty());
}

#[tokio::test]
async fn recent_duplicate_finds_match_within_window() {
    let (_tmp, svc) = make_service();
    svc.add("AAPL", TrackerSource::Manual, None, vec![], None)
        .await
        .unwrap();
    let cand = sample_candidate();
    let inserted = svc.insert_setup("AAPL", &cand).await.unwrap();

    let dup = svc
        .recent_duplicate(
            "AAPL",
            "breakout",
            Direction::Long,
            ChronoDuration::hours(24),
        )
        .await
        .unwrap();
    assert_eq!(dup, Some(inserted.id));

    // Different direction → no match.
    let dup_short = svc
        .recent_duplicate(
            "AAPL",
            "breakout",
            Direction::Short,
            ChronoDuration::hours(24),
        )
        .await
        .unwrap();
    assert!(dup_short.is_none());

    // Different strategy → no match.
    let dup_other = svc
        .recent_duplicate(
            "AAPL",
            "episodic_pivot",
            Direction::Long,
            ChronoDuration::hours(24),
        )
        .await
        .unwrap();
    assert!(dup_other.is_none());
}

#[tokio::test]
async fn recent_duplicate_returns_none_for_old_row() {
    let (_tmp, svc) = make_service();
    svc.add("AAPL", TrackerSource::Manual, None, vec![], None)
        .await
        .unwrap();
    let mut cand = sample_candidate();
    cand.detected_at = Utc::now() - ChronoDuration::days(2);
    svc.insert_setup("AAPL", &cand).await.unwrap();

    let dup = svc
        .recent_duplicate(
            "AAPL",
            "breakout",
            Direction::Long,
            ChronoDuration::hours(24),
        )
        .await
        .unwrap();
    assert!(dup.is_none());
}

#[tokio::test]
async fn get_setup_returns_none_for_missing_id() {
    let (_tmp, svc) = make_service();
    assert!(svc.get_setup(9999).await.unwrap().is_none());
}
