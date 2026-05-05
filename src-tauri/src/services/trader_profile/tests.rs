//! Unit tests for the trader_profile aggregator. Seeds rows directly via
//! SQL so the tests don't depend on the full eod_review chain.

use std::sync::Arc;

use chrono::{Duration, NaiveDate, Utc};
use chrono_tz::America::New_York;
use rusqlite::params;
use serde_json::json;

use crate::mcp::tools::test_support::make_db;
use crate::services::trade_reviews::tags::BehavioralTag;
use crate::storage::Db;

use super::aggregate;

fn today_et() -> NaiveDate {
    Utc::now().with_timezone(&New_York).date_naive()
}

async fn seed_review(
    db: &Arc<Db>,
    date: NaiveDate,
    account: &str,
    tags: &[BehavioralTag],
    net_pnl: f64,
    grade_score: f64,
    leg_observations: &[serde_json::Value],
) {
    let date_str = date.to_string();
    let account = account.to_string();
    let tags_json = serde_json::to_string(tags).unwrap();
    let legs_json = serde_json::to_string(leg_observations).unwrap();
    let summary_json = json!({
        "gross_pnl": net_pnl,
        "net_pnl": net_pnl,
        "commissions_total": 0.0,
        "n_round_trips": 0,
        "n_carryover": 0,
        "win_rate": null,
        "by_symbol": {}
    })
    .to_string();
    let now = Utc::now().to_rfc3339();
    db.with_conn(move |conn| {
        conn.execute(
            "INSERT OR REPLACE INTO day_reviews (
                date, account, prompt_version, generated_at, grade, grade_score,
                gross_pnl, net_pnl, commissions_total, n_round_trips, n_carryover,
                win_rate, behavioral_tags, leg_observations, summary_json,
                narrative_md, llm_call_id
             ) VALUES (
                ?1, ?2, 1, ?3, 'C', ?4, ?5, ?5, 0.0, 0, 0,
                NULL, ?6, ?7, ?8, '', NULL
             )",
            params![
                date_str,
                account,
                now,
                grade_score,
                net_pnl,
                tags_json,
                legs_json,
                summary_json,
            ],
        )?;
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn aggregator_empty_store_returns_zero_review_profile() {
    let (_tmp, db) = make_db();
    let p = aggregate(&db, "U1", 30).await.expect("ok");
    assert_eq!(p.n_reviews, 0);
    assert!(p.tag_frequencies.is_empty());
    assert!(p.pnl_by_tag.is_empty());
    assert_eq!(p.trendline.last_7d.n_reviews, 0);
    assert_eq!(p.trendline.prior_21d.n_reviews, 0);
    assert!(p.recent_incidents.is_empty());
    assert_eq!(p.window_days, 30);
    assert_eq!(p.account, "U1");
}

#[tokio::test]
async fn aggregator_counts_tags_across_window() {
    let (_tmp, db) = make_db();
    let today = today_et();
    seed_review(
        &db,
        today - Duration::days(1),
        "U1",
        &[BehavioralTag::FlatClose, BehavioralTag::ChaseOwnExit],
        100.0,
        5.0,
        &[],
    )
    .await;
    seed_review(
        &db,
        today - Duration::days(2),
        "U1",
        &[BehavioralTag::FlatClose],
        200.0,
        10.0,
        &[],
    )
    .await;
    seed_review(
        &db,
        today - Duration::days(3),
        "U1",
        &[
            BehavioralTag::ChaseOwnExit,
            BehavioralTag::DisciplineOnLoser,
        ],
        -50.0,
        -3.0,
        &[],
    )
    .await;

    let p = aggregate(&db, "U1", 30).await.unwrap();
    assert_eq!(p.n_reviews, 3);

    let flat = p
        .tag_frequencies
        .iter()
        .find(|f| matches!(f.tag, BehavioralTag::FlatClose))
        .expect("flat_close");
    assert_eq!(flat.count, 2);
    assert!((flat.pct_of_reviews - 2.0 / 3.0).abs() < 1e-9);

    let chase = p
        .tag_frequencies
        .iter()
        .find(|f| matches!(f.tag, BehavioralTag::ChaseOwnExit))
        .expect("chase");
    assert_eq!(chase.count, 2);

    // pnl_by_tag aggregates the net_pnl of every day a tag fired.
    let chase_pnl = p
        .pnl_by_tag
        .iter()
        .find(|p| matches!(p.tag, BehavioralTag::ChaseOwnExit))
        .expect("chase pnl");
    assert!((chase_pnl.net_pnl_total - 50.0).abs() < 1e-9); // 100 + (-50)
    assert_eq!(chase_pnl.n_days, 2);
}

#[tokio::test]
async fn aggregator_isolates_account_window() {
    let (_tmp, db) = make_db();
    let today = today_et();
    seed_review(
        &db,
        today - Duration::days(1),
        "U1",
        &[BehavioralTag::FlatClose],
        100.0,
        5.0,
        &[],
    )
    .await;
    seed_review(
        &db,
        today - Duration::days(1),
        "U2",
        &[BehavioralTag::ChaseOwnExit],
        -100.0,
        -10.0,
        &[],
    )
    .await;

    let p1 = aggregate(&db, "U1", 30).await.unwrap();
    assert_eq!(p1.n_reviews, 1);
    assert!(p1
        .tag_frequencies
        .iter()
        .all(|f| !matches!(f.tag, BehavioralTag::ChaseOwnExit)));

    let p2 = aggregate(&db, "U2", 30).await.unwrap();
    assert_eq!(p2.n_reviews, 1);
    assert!(p2
        .tag_frequencies
        .iter()
        .all(|f| !matches!(f.tag, BehavioralTag::FlatClose)));
}

#[tokio::test]
async fn aggregator_trendline_splits_last_7_vs_prior_21() {
    let (_tmp, db) = make_db();
    let today = today_et();
    // 5 reviews in last 7d (days 1..=5)
    for d in 1..=5 {
        seed_review(
            &db,
            today - Duration::days(d),
            "U1",
            &[BehavioralTag::FlatClose],
            50.0,
            5.0,
            &[],
        )
        .await;
    }
    // 10 reviews in prior 21d (days 8..=17 — strictly older than 7d, within 28d)
    for d in 8..=17 {
        seed_review(
            &db,
            today - Duration::days(d),
            "U1",
            &[BehavioralTag::FlatClose],
            30.0,
            3.0,
            &[],
        )
        .await;
    }
    let p = aggregate(&db, "U1", 30).await.unwrap();
    assert_eq!(p.trendline.last_7d.n_reviews, 5);
    assert_eq!(p.trendline.prior_21d.n_reviews, 10);
    assert!((p.trendline.last_7d.net_pnl - 250.0).abs() < 1e-9);
    assert!((p.trendline.prior_21d.net_pnl - 300.0).abs() < 1e-9);
    assert!((p.trendline.last_7d.avg_grade_score - 5.0).abs() < 1e-9);
    assert!((p.trendline.prior_21d.avg_grade_score - 3.0).abs() < 1e-9);
    assert_eq!(
        p.trendline.last_7d.tag_counts.get("flat_close").copied(),
        Some(5)
    );
    assert_eq!(
        p.trendline.prior_21d.tag_counts.get("flat_close").copied(),
        Some(10)
    );
}

#[tokio::test]
async fn aggregator_filters_rows_outside_window() {
    let (_tmp, db) = make_db();
    let today = today_et();
    // Inside window (10d back).
    seed_review(
        &db,
        today - Duration::days(10),
        "U1",
        &[BehavioralTag::FlatClose],
        100.0,
        5.0,
        &[],
    )
    .await;
    // Outside the 7-day window the caller asked for.
    seed_review(
        &db,
        today - Duration::days(20),
        "U1",
        &[BehavioralTag::ChaseOwnExit],
        -100.0,
        -10.0,
        &[],
    )
    .await;

    let p = aggregate(&db, "U1", 7).await.unwrap();
    assert_eq!(p.n_reviews, 0); // both rows older than 7 days
    let p2 = aggregate(&db, "U1", 15).await.unwrap();
    assert_eq!(p2.n_reviews, 1);
}

#[tokio::test]
async fn aggregator_recent_incidents_carry_symbol_and_tag() {
    let (_tmp, db) = make_db();
    let today = today_et();
    let legs = vec![
        json!({
            "leg_id": "TSLA-001",
            "symbol": "TSLA",
            "observation_md": "re-entered 395C at $2.50 within 2 min of selling at $2.45",
            "tag": "chase_own_exit"
        }),
        json!({
            "leg_id": "AMD-001",
            "symbol": "AMD",
            "observation_md": "scaled in on weakness",
            "tag": "scaled_in_loser"
        }),
    ];
    seed_review(
        &db,
        today - Duration::days(2),
        "U1",
        &[BehavioralTag::ChaseOwnExit, BehavioralTag::ScaledInLoser],
        -250.0,
        -8.0,
        &legs,
    )
    .await;

    let p = aggregate(&db, "U1", 30).await.unwrap();
    assert_eq!(p.recent_incidents.len(), 2);
    let tsla = p
        .recent_incidents
        .iter()
        .find(|i| i.symbol == "TSLA")
        .expect("tsla incident");
    assert!(matches!(tsla.tag, BehavioralTag::ChaseOwnExit));
    assert_eq!(tsla.date, today - Duration::days(2));
    assert!(tsla.leg_observation.contains("re-entered"));
}

#[tokio::test]
async fn aggregator_recent_incidents_skipped_outside_last_7d() {
    let (_tmp, db) = make_db();
    let today = today_et();
    let legs = vec![json!({
        "leg_id": "TSLA-001",
        "symbol": "TSLA",
        "observation_md": "old chase",
        "tag": "chase_own_exit"
    })];
    seed_review(
        &db,
        today - Duration::days(10),
        "U1",
        &[BehavioralTag::ChaseOwnExit],
        -100.0,
        -5.0,
        &legs,
    )
    .await;

    let p = aggregate(&db, "U1", 30).await.unwrap();
    // Tag still counts in tag_frequencies, but no incident surfaced.
    assert_eq!(p.n_reviews, 1);
    assert!(p.recent_incidents.is_empty());
}

#[tokio::test]
async fn aggregator_recent_incidents_capped() {
    let (_tmp, db) = make_db();
    let today = today_et();
    // 12 legs in one review; cap is 10.
    let legs: Vec<serde_json::Value> = (0..12)
        .map(|i| {
            json!({
                "leg_id": format!("S-{i:03}"),
                "symbol": format!("SYM{i}"),
                "observation_md": "x",
                "tag": "chase_own_exit"
            })
        })
        .collect();
    seed_review(
        &db,
        today - Duration::days(1),
        "U1",
        &[BehavioralTag::ChaseOwnExit],
        -100.0,
        -5.0,
        &legs,
    )
    .await;

    let p = aggregate(&db, "U1", 30).await.unwrap();
    assert_eq!(p.recent_incidents.len(), 10);
}

#[tokio::test]
async fn aggregator_drops_unknown_tags_silently() {
    let (_tmp, db) = make_db();
    let today = today_et();
    // Mix a known tag with an unknown one a v2 prompt_version might emit.
    let date_str = (today - Duration::days(1)).to_string();
    let now = Utc::now().to_rfc3339();
    let summary_json = json!({
        "gross_pnl": 100.0,
        "net_pnl": 100.0,
        "commissions_total": 0.0,
        "n_round_trips": 0,
        "n_carryover": 0,
        "win_rate": null,
        "by_symbol": {}
    })
    .to_string();
    let tags_json = json!(["flat_close", "future_unknown_tag"]).to_string();
    let legs_json = "[]".to_string();
    db.with_conn(move |conn| {
        conn.execute(
            "INSERT OR REPLACE INTO day_reviews (
                date, account, prompt_version, generated_at, grade, grade_score,
                gross_pnl, net_pnl, commissions_total, n_round_trips, n_carryover,
                win_rate, behavioral_tags, leg_observations, summary_json,
                narrative_md, llm_call_id
             ) VALUES (?1, 'U1', 1, ?2, 'C', 5.0, 100.0, 100.0, 0.0, 0, 0,
                       NULL, ?3, ?4, ?5, '', NULL)",
            params![date_str, now, tags_json, legs_json, summary_json],
        )?;
        Ok(())
    })
    .await
    .unwrap();

    let p = aggregate(&db, "U1", 30).await.unwrap();
    assert_eq!(p.n_reviews, 1);
    // Unknown tag silently dropped; flat_close survives.
    assert_eq!(p.tag_frequencies.len(), 1);
    assert!(matches!(p.tag_frequencies[0].tag, BehavioralTag::FlatClose));
}
