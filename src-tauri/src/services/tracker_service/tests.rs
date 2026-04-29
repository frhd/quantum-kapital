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

    svc.set_status("MSFT", TrackerStatus::InPlay, None)
        .await
        .unwrap();
    svc.set_status("NVDA", TrackerStatus::SetupActive, None)
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
        .set_status("AAPL", TrackerStatus::InPlay, Some(until))
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
