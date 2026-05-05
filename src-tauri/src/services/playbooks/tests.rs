//! Integration tests for `PlaybookStore`. Schema round-trip lives next
//! to the types it pins.

use chrono::NaiveDate;

use crate::mcp::tools::test_support::make_db;

use super::types::{
    Conviction, EvidenceRef, Playbook, RankedSetup, SetupBias, SkipEntry, WritePlaybookRequest,
};
use super::PlaybookStore;

fn sample_setup(symbol: &str, bias: SetupBias, conviction: Conviction) -> RankedSetup {
    RankedSetup {
        symbol: symbol.into(),
        bias,
        trigger: "reclaim of 5/4 HOD on volume".into(),
        entry: "$165–166".into(),
        invalidation: "lose $164".into(),
        target_1: "$172".into(),
        target_2: Some("$178".into()),
        conviction,
        rationale_md: "Catalyst + base.".into(),
        evidence_refs: vec![EvidenceRef {
            source: "news".into(),
            note: "filing 8-K positive".into(),
        }],
    }
}

fn sample_request(date: &str, account: &str) -> WritePlaybookRequest {
    WritePlaybookRequest {
        date: NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap(),
        account: account.into(),
        ranked_setups: vec![
            sample_setup("TSLA", SetupBias::Long, Conviction::A),
            sample_setup("NVDA", SetupBias::Short, Conviction::B),
        ],
        skip_list: vec![SkipEntry {
            symbol: "AAPL".into(),
            reason: "earnings AMC".into(),
        }],
        llm_call_id: Some("llm-call-9".into()),
    }
}

#[tokio::test]
async fn store_write_returns_monotonic_generation_id() {
    let (_tmp, db) = make_db();
    let store = PlaybookStore::new(db);
    let req = sample_request("2026-05-05", "U1");
    let g1 = store.write(req.clone()).await.unwrap();
    let g2 = store.write(req.clone()).await.unwrap();
    assert_eq!(g1.playbook.generation_id, 1);
    assert_eq!(g2.playbook.generation_id, 2);
}

#[tokio::test]
async fn store_read_latest_returns_most_recent_generation() {
    let (_tmp, db) = make_db();
    let store = PlaybookStore::new(db);
    store.write(sample_request("2026-05-05", "U1")).await.unwrap();
    store.write(sample_request("2026-05-05", "U1")).await.unwrap(); // generation 2
    let pb = store
        .read_latest(NaiveDate::from_ymd_opt(2026, 5, 5).unwrap(), "U1")
        .await
        .unwrap()
        .expect("playbook");
    assert_eq!(pb.generation_id, 2);
    assert_eq!(pb.ranked_setups.len(), 2);
    assert_eq!(pb.ranked_setups[0].symbol, "TSLA");
    assert_eq!(pb.skip_list.len(), 1);
    assert_eq!(pb.llm_call_id.as_deref(), Some("llm-call-9"));
}

#[tokio::test]
async fn store_read_specific_generation_returns_that_one() {
    let (_tmp, db) = make_db();
    let store = PlaybookStore::new(db);
    store.write(sample_request("2026-05-05", "U1")).await.unwrap();
    store.write(sample_request("2026-05-05", "U1")).await.unwrap();
    let g1 = store
        .read_generation(NaiveDate::from_ymd_opt(2026, 5, 5).unwrap(), "U1", 1)
        .await
        .unwrap()
        .expect("g1");
    assert_eq!(g1.generation_id, 1);
}

#[tokio::test]
async fn store_read_missing_returns_none() {
    let (_tmp, db) = make_db();
    let store = PlaybookStore::new(db);
    let pb = store
        .read_latest(NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(), "U1")
        .await
        .unwrap();
    assert!(pb.is_none());
}

#[tokio::test]
async fn store_separate_dates_have_independent_generations() {
    let (_tmp, db) = make_db();
    let store = PlaybookStore::new(db);
    let g1a = store.write(sample_request("2026-05-04", "U1")).await.unwrap();
    let g1b = store.write(sample_request("2026-05-05", "U1")).await.unwrap();
    assert_eq!(g1a.playbook.generation_id, 1);
    assert_eq!(g1b.playbook.generation_id, 1);
}

#[tokio::test]
async fn store_separate_accounts_have_independent_generations() {
    let (_tmp, db) = make_db();
    let store = PlaybookStore::new(db);
    let g1a = store.write(sample_request("2026-05-05", "U1")).await.unwrap();
    let g1b = store.write(sample_request("2026-05-05", "U2")).await.unwrap();
    assert_eq!(g1a.playbook.generation_id, 1);
    assert_eq!(g1b.playbook.generation_id, 1);
}

#[tokio::test]
async fn store_rejects_empty_account() {
    let (_tmp, db) = make_db();
    let store = PlaybookStore::new(db);
    let mut req = sample_request("2026-05-05", "  ");
    req.account = "  ".into();
    let err = store.write(req).await.expect_err("rejects empty account");
    assert!(matches!(err, super::PlaybookError::EmptyAccount));
}

#[tokio::test]
async fn store_count_tracks_writes() {
    let (_tmp, db) = make_db();
    let store = PlaybookStore::new(db);
    assert_eq!(store.count().await.unwrap(), 0);
    store.write(sample_request("2026-05-05", "U1")).await.unwrap();
    store.write(sample_request("2026-05-05", "U1")).await.unwrap();
    assert_eq!(store.count().await.unwrap(), 2);
}

/// Pins the wire shape so accidental field renames break here first.
/// Mirrors the round-trip discipline used in `trade_reviews` (Phase 4).
#[test]
fn playbook_serde_round_trip_preserves_all_fields() {
    let pb = Playbook {
        date: NaiveDate::from_ymd_opt(2026, 5, 5).unwrap(),
        account: "U1234567".into(),
        generation_id: 3,
        generated_at: chrono::DateTime::parse_from_rfc3339("2026-05-05T11:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc),
        ranked_setups: vec![sample_setup("TSLA", SetupBias::Long, Conviction::A)],
        skip_list: vec![SkipEntry {
            symbol: "AAPL".into(),
            reason: "earnings AMC".into(),
        }],
        llm_call_id: Some("llm-call-9".into()),
    };
    let json = serde_json::to_string(&pb).expect("serialise");
    let back: Playbook = serde_json::from_str(&json).expect("deserialise");
    assert_eq!(back, pb);
}
