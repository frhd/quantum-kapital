use std::sync::Arc;

use chrono::{NaiveDate, TimeZone, Utc};
use tempfile::NamedTempFile;

use crate::services::agent_morning_packs::{self, NewAgentMorningPack, RankedIdea};
use crate::services::eval_harness::{calibration_stats, cost_attribution, prediction_history};
use crate::services::outcome_extractor::{record_outcome, NewOutcome, OutcomeClass};
use crate::services::predictions::find_for_pack;
use crate::services::research_notes::Conviction;
use crate::storage::Db;

fn open_db() -> (NamedTempFile, Arc<Db>) {
    let tmp = NamedTempFile::new().expect("tempfile");
    let db = Db::open(tmp.path()).expect("open db");
    (tmp, Arc::new(db))
}

fn date(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

fn idea(symbol: &str, conv: Conviction) -> RankedIdea {
    RankedIdea {
        symbol: symbol.to_string(),
        thesis_md: format!("thesis for {symbol}"),
        conviction: Some(conv),
        entry_zone: Some("100-105".to_string()),
        invalidation: Some("close < 95".to_string()),
        evidence_refs: vec![],
    }
}

async fn seed_pack(db: &Arc<Db>, pack_date: NaiveDate, ideas: Vec<RankedIdea>) {
    agent_morning_packs::write_pack(
        db,
        NewAgentMorningPack {
            date: pack_date,
            ranked_ideas: ideas,
            written_by: "agent_morning_sweep".to_string(),
        },
    )
    .await
    .unwrap();
}

async fn record(
    db: &Arc<Db>,
    pack_date: NaiveDate,
    symbol: &str,
    class: OutcomeClass,
    conviction: Option<Conviction>,
) {
    let pred = find_for_pack(db, &pack_date.to_string(), symbol)
        .await
        .unwrap();
    record_outcome(
        db,
        NewOutcome {
            pack_date,
            symbol: symbol.to_string(),
            outcome_class: class,
            conviction,
            entry_zone_low: Some(100.0),
            entry_zone_high: Some(105.0),
            invalidation_lvl: Some(95.0),
            realized_high: 110.0,
            realized_low: 99.0,
            realized_close: 104.0,
            eval_window_days: 1,
            prediction_id: pred.map(|p| p.id),
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn calibration_stats_buckets_by_conviction() {
    let (_tmp, db) = open_db();
    let pack_date = date("2026-05-01");
    seed_pack(
        &db,
        pack_date,
        vec![
            idea("TSLA", Conviction::A),
            idea("AAPL", Conviction::A),
            idea("MSFT", Conviction::B),
            idea("NVDA", Conviction::C),
        ],
    )
    .await;

    record(
        &db,
        pack_date,
        "TSLA",
        OutcomeClass::HitTarget,
        Some(Conviction::A),
    )
    .await;
    record(
        &db,
        pack_date,
        "AAPL",
        OutcomeClass::HitInvalidation,
        Some(Conviction::A),
    )
    .await;
    record(
        &db,
        pack_date,
        "MSFT",
        OutcomeClass::HitEntry,
        Some(Conviction::B),
    )
    .await;
    record(
        &db,
        pack_date,
        "NVDA",
        OutcomeClass::Drifted,
        Some(Conviction::C),
    )
    .await;

    let stats = calibration_stats(&db, 30, 0).await.unwrap();
    let a = stats
        .buckets
        .iter()
        .find(|b| b.conviction.as_deref() == Some("A"))
        .unwrap();
    assert_eq!(a.total, 2);
    assert_eq!(a.hit_target, 1);
    assert_eq!(a.hit_invalidation, 1);
    assert!((a.win_rate - 0.5).abs() < 1e-9);
    assert!((a.target_rate - 0.5).abs() < 1e-9);

    let b = stats
        .buckets
        .iter()
        .find(|x| x.conviction.as_deref() == Some("B"))
        .unwrap();
    assert_eq!(b.hit_entry, 1);
    assert!((b.win_rate - 1.0).abs() < 1e-9);

    assert_eq!(stats.overall.total, 4);
}

#[tokio::test]
async fn calibration_stats_excludes_skipped_from_rates() {
    let (_tmp, db) = open_db();
    let pack_date = date("2026-05-02");
    seed_pack(
        &db,
        pack_date,
        vec![idea("TSLA", Conviction::A), idea("AAPL", Conviction::A)],
    )
    .await;

    record(
        &db,
        pack_date,
        "TSLA",
        OutcomeClass::HitTarget,
        Some(Conviction::A),
    )
    .await;
    record(
        &db,
        pack_date,
        "AAPL",
        OutcomeClass::Skipped,
        Some(Conviction::A),
    )
    .await;

    let stats = calibration_stats(&db, 30, 0).await.unwrap();
    let a = stats
        .buckets
        .iter()
        .find(|b| b.conviction.as_deref() == Some("A"))
        .unwrap();
    assert_eq!(a.total, 2);
    assert_eq!(a.skipped, 1);
    // Only the non-skipped row counts towards the rate.
    assert!((a.win_rate - 1.0).abs() < 1e-9);
}

#[tokio::test]
async fn cost_attribution_buckets_by_loop_or_kind() {
    let (_tmp, db) = open_db();
    let now = 1_700_000_000i64;
    db.with_conn(move |conn| {
        // Two loop-tagged rows + one un-tagged row.
        conn.execute(
            "INSERT INTO llm_calls \
             (kind, model, input_tokens, output_tokens, cache_read_tokens, cost_usd, \
              called_at, loop_name) \
             VALUES \
             ('news', 'claude-sonnet-4-6', 100, 50, 0, 0.10, ?1, 'agent_morning_sweep'), \
             ('news', 'claude-sonnet-4-6', 100, 50, 0, 0.20, ?1, 'agent_morning_sweep'), \
             ('thesis', 'claude-sonnet-4-6', 100, 50, 0, 0.05, ?1, NULL)",
            rusqlite::params![now],
        )?;
        Ok(())
    })
    .await
    .unwrap();

    let stats = cost_attribution(&db, 1, 0).await.unwrap();
    assert!((stats.total_cost_usd - 0.35).abs() < 1e-9);
    assert_eq!(stats.total_calls, 3);
    let sweep = stats
        .buckets
        .iter()
        .find(|b| b.bucket == "agent_morning_sweep")
        .unwrap();
    assert_eq!(sweep.call_count, 2);
    assert!((sweep.cost_usd - 0.30).abs() < 1e-9);
    let kind = stats
        .buckets
        .iter()
        .find(|b| b.bucket == "kind:thesis")
        .unwrap();
    assert_eq!(kind.call_count, 1);
}

#[tokio::test]
async fn cost_attribution_usd_per_a_conviction_with_no_a_calls_is_nan() {
    let (_tmp, db) = open_db();
    let stats = cost_attribution(&db, 30, 0).await.unwrap();
    assert_eq!(stats.a_conviction_count, 0);
    assert!(stats.usd_per_a_conviction.is_nan());
}

#[tokio::test]
async fn prediction_history_joins_outcome_when_present() {
    let (_tmp, db) = open_db();
    let pack_date = date("2026-05-03");
    seed_pack(
        &db,
        pack_date,
        vec![idea("TSLA", Conviction::A), idea("AAPL", Conviction::B)],
    )
    .await;

    record(
        &db,
        pack_date,
        "TSLA",
        OutcomeClass::HitTarget,
        Some(Conviction::A),
    )
    .await;
    // AAPL has no outcome row.

    let pack_date2 = date("2026-05-04");
    seed_pack(&db, pack_date2, vec![idea("TSLA", Conviction::C)]).await;

    let hist = prediction_history(&db, "tsla", 0).await.unwrap();
    assert_eq!(hist.len(), 2);
    // newest first
    assert_eq!(
        hist[0].prediction.morning_pack_id.as_deref(),
        Some("2026-05-04")
    );
    assert!(hist[0].outcome.is_none());
    assert_eq!(
        hist[1].prediction.morning_pack_id.as_deref(),
        Some("2026-05-03")
    );
    let outcome = hist[1].outcome.as_ref().expect("outcome present");
    assert_eq!(outcome.outcome_class, OutcomeClass::HitTarget);

    let aapl_hist = prediction_history(&db, "AAPL", 0).await.unwrap();
    assert_eq!(aapl_hist.len(), 1);
    assert!(aapl_hist[0].outcome.is_none());
}

#[tokio::test]
async fn calibration_stats_respects_since_unix_window() {
    let (_tmp, db) = open_db();
    let old_pack = date("2026-04-01");
    let recent_pack = date("2026-04-30");

    // Old pack with custom predicted_at (90 days ago).
    seed_pack(&db, old_pack, vec![idea("TSLA", Conviction::A)]).await;
    let old_unix = Utc
        .with_ymd_and_hms(2026, 1, 1, 0, 0, 0)
        .unwrap()
        .timestamp();
    db.with_conn(move |conn| {
        conn.execute(
            "UPDATE predictions SET predicted_at = ?1 WHERE morning_pack_id = '2026-04-01'",
            rusqlite::params![old_unix],
        )?;
        Ok(())
    })
    .await
    .unwrap();
    record(
        &db,
        old_pack,
        "TSLA",
        OutcomeClass::HitTarget,
        Some(Conviction::A),
    )
    .await;

    seed_pack(&db, recent_pack, vec![idea("AAPL", Conviction::A)]).await;
    record(
        &db,
        recent_pack,
        "AAPL",
        OutcomeClass::HitInvalidation,
        Some(Conviction::A),
    )
    .await;

    let cutoff = Utc
        .with_ymd_and_hms(2026, 4, 15, 0, 0, 0)
        .unwrap()
        .timestamp();
    let stats = calibration_stats(&db, 30, cutoff).await.unwrap();
    assert_eq!(
        stats.overall.total, 1,
        "only the recent pack falls in window"
    );
    let a = stats
        .buckets
        .iter()
        .find(|b| b.conviction.as_deref() == Some("A"))
        .unwrap();
    assert_eq!(a.hit_invalidation, 1);
    assert_eq!(a.hit_target, 0);
}
