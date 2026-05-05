//! Integration tests for `TradeReviewStore`. v2-scoring unit tests
//! live in `grade.rs` / `risk_metrics.rs` / `equity_curve.rs`; this
//! file exercises the persistence round-trip with the Phase 4
//! signature `store.write(req, v2_fields)`.

use std::collections::BTreeMap;

use chrono::NaiveDate;

use crate::mcp::tools::test_support::make_db;

use super::equity_curve::EquityPoint;
use super::risk_metrics::{RiskMetrics, DEFAULT_RISK_FREE_RATE_ANNUAL};
use super::tags::BehavioralTag;
use super::types::{LegObservation, LegSummary, ReviewV2Fields, WriteTradeReviewRequest};
use super::TradeReviewStore;

fn sample_summary() -> LegSummary {
    let mut by_symbol = BTreeMap::new();
    by_symbol.insert("TSLA".to_string(), 250.0);
    by_symbol.insert("AAPL".to_string(), 130.0);
    LegSummary {
        gross_pnl: 401.10,
        net_pnl: 380.0,
        commissions_total: 21.10,
        n_round_trips: 3,
        n_carryover: 0,
        win_rate: Some(2.0 / 3.0),
        by_symbol,
    }
}

fn sample_request() -> WriteTradeReviewRequest {
    WriteTradeReviewRequest {
        date: NaiveDate::from_ymd_opt(2026, 5, 4).unwrap(),
        account: "U1234567".into(),
        prompt_version: 1,
        summary: sample_summary(),
        behavioral_tags: vec![BehavioralTag::FlatClose, BehavioralTag::DisciplineOnLoser],
        leg_observations: vec![LegObservation {
            leg_id: "L1".into(),
            symbol: Some("AAPL".into()),
            observation_md: "Best leg of the day.".into(),
            tag: Some(BehavioralTag::DisciplineOnLoser),
        }],
        narrative_md: "Solid disciplined day.".into(),
        llm_call_id: Some("llm-call-7".into()),
    }
}

fn sample_v2_fields() -> ReviewV2Fields {
    ReviewV2Fields {
        score_v2: Some(2.5),
        discipline_v2: Some(10.0),
        risk_metrics: Some(RiskMetrics::empty(DEFAULT_RISK_FREE_RATE_ANNUAL)),
        equity_curve: Some(vec![EquityPoint {
            date: NaiveDate::from_ymd_opt(2026, 5, 4).unwrap(),
            equity: 100_098.0,
            daily_pnl: 98.0,
        }]),
        formula_version: "v2".into(),
    }
}

#[tokio::test]
async fn store_writes_and_reads_review() {
    let (_tmp, db) = make_db();
    let store = TradeReviewStore::new(db);
    let req = sample_request();

    let outcome = store
        .write(req.clone(), sample_v2_fields())
        .await
        .expect("write ok");
    assert_eq!(outcome.review.behavioral_tags, req.behavioral_tags);
    assert_eq!(outcome.review.narrative_md, "Solid disciplined day.");
    assert_eq!(outcome.formula_version, "v2");
    assert_eq!(outcome.score_v2, Some(2.5));
    assert_eq!(outcome.discipline_v2, Some(10.0));
    assert!(outcome.review.grade.is_none(), "v2 rows leave legacy grade NULL");

    let row = store
        .read(req.date, &req.account, req.prompt_version)
        .await
        .expect("read ok")
        .expect("row exists");
    assert_eq!(row.date, req.date);
    assert_eq!(row.formula_version, "v2");
    assert_eq!(row.score_v2, Some(2.5));
    assert_eq!(row.discipline_v2, Some(10.0));
    assert!(row.risk_metrics.is_some());
    assert_eq!(row.equity_curve.as_ref().map(|c| c.len()), Some(1));
    assert_eq!(row.behavioral_tags, req.behavioral_tags);
    assert_eq!(row.leg_observations.len(), 1);
    assert_eq!(row.summary, sample_summary());
}

#[tokio::test]
async fn store_upserts_review_idempotently() {
    let (_tmp, db) = make_db();
    let store = TradeReviewStore::new(db);
    let req = sample_request();

    store
        .write(req.clone(), sample_v2_fields())
        .await
        .expect("first");
    let mut second = req.clone();
    second.narrative_md = "Updated narrative.".into();
    store
        .write(second.clone(), sample_v2_fields())
        .await
        .expect("second");

    let count = store.count().await.expect("count");
    assert_eq!(count, 1, "expected 1 row after idempotent upsert");

    let row = store
        .read(req.date, &req.account, req.prompt_version)
        .await
        .expect("read")
        .expect("row");
    assert_eq!(row.narrative_md, "Updated narrative.");
}

#[tokio::test]
async fn store_separate_rows_per_prompt_version() {
    let (_tmp, db) = make_db();
    let store = TradeReviewStore::new(db);
    let mut req = sample_request();
    req.prompt_version = 1;
    store
        .write(req.clone(), sample_v2_fields())
        .await
        .unwrap();
    req.prompt_version = 2;
    req.narrative_md = "v2 narrative".into();
    store
        .write(req.clone(), sample_v2_fields())
        .await
        .unwrap();

    let count = store.count().await.expect("count");
    assert_eq!(count, 2);

    let r1 = store
        .read(req.date, &req.account, 1)
        .await
        .unwrap()
        .expect("v1");
    let r2 = store
        .read(req.date, &req.account, 2)
        .await
        .unwrap()
        .expect("v2");
    assert_eq!(r1.prompt_version, 1);
    assert_eq!(r2.prompt_version, 2);
    assert_eq!(r2.narrative_md, "v2 narrative");
}

#[tokio::test]
async fn store_read_latest_returns_highest_prompt_version() {
    let (_tmp, db) = make_db();
    let store = TradeReviewStore::new(db);
    let mut req = sample_request();
    req.prompt_version = 1;
    store
        .write(req.clone(), sample_v2_fields())
        .await
        .unwrap();
    req.prompt_version = 5;
    req.narrative_md = "v5".into();
    store
        .write(req.clone(), sample_v2_fields())
        .await
        .unwrap();
    req.prompt_version = 3;
    req.narrative_md = "v3".into();
    store
        .write(req.clone(), sample_v2_fields())
        .await
        .unwrap();

    let latest = store
        .read_latest(req.date, &req.account)
        .await
        .unwrap()
        .expect("latest exists");
    assert_eq!(latest.prompt_version, 5);
    assert_eq!(latest.narrative_md, "v5");
}

#[tokio::test]
async fn store_read_missing_returns_none() {
    let (_tmp, db) = make_db();
    let store = TradeReviewStore::new(db);
    let row = store
        .read(NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(), "missing", 1)
        .await
        .unwrap();
    assert!(row.is_none());
}

#[tokio::test]
async fn store_rejects_empty_account() {
    let (_tmp, db) = make_db();
    let store = TradeReviewStore::new(db);
    let mut req = sample_request();
    req.account = "  ".into();
    let err = store
        .write(req, sample_v2_fields())
        .await
        .expect_err("rejects empty account");
    assert!(matches!(err, super::TradeReviewError::EmptyAccount));
}

#[tokio::test]
async fn store_rejects_empty_narrative() {
    let (_tmp, db) = make_db();
    let store = TradeReviewStore::new(db);
    let mut req = sample_request();
    req.narrative_md = "".into();
    let err = store
        .write(req, sample_v2_fields())
        .await
        .expect_err("rejects empty narrative");
    assert!(matches!(err, super::TradeReviewError::EmptyNarrative));
}

#[tokio::test]
async fn store_v1_only_passthrough_writes_legacy_row() {
    // Pre-P4 callers (or callers that opted out) get a row tagged
    // `formula_version='v1'` with NULL v2 numerics — never silently
    // upgraded to v2. Verifies the read-back path tolerates it.
    let (_tmp, db) = make_db();
    let store = TradeReviewStore::new(db);
    let req = sample_request();
    let outcome = store
        .write(req.clone(), ReviewV2Fields::v1_only())
        .await
        .expect("write ok");
    assert_eq!(outcome.formula_version, "v1");
    assert!(outcome.score_v2.is_none());
    let row = store
        .read(req.date, &req.account, req.prompt_version)
        .await
        .unwrap()
        .expect("row");
    assert_eq!(row.formula_version, "v1");
    assert!(row.score_v2.is_none());
    assert!(row.discipline_v2.is_none());
    assert!(row.risk_metrics.is_none());
}
