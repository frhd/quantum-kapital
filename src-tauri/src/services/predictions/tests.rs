use std::sync::Arc;

use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use tempfile::NamedTempFile;

use crate::services::agent_morning_packs::{AgentMorningPack, RankedIdea};
use crate::services::predictions::{
    find_for_pack, list_predictions, record_predictions_from_pack, SOURCE_AGENT_MORNING_SWEEP,
};
use crate::services::research_notes::Conviction;
use crate::storage::Db;

fn open_db() -> (NamedTempFile, Arc<Db>) {
    let tmp = NamedTempFile::new().expect("tempfile");
    let db = Db::open(tmp.path()).expect("open db");
    (tmp, Arc::new(db))
}

fn pack_at(date: NaiveDate, written_at: DateTime<Utc>, ideas: Vec<RankedIdea>) -> AgentMorningPack {
    AgentMorningPack {
        date,
        ranked_ideas: ideas,
        written_by: "agent_morning_sweep".to_string(),
        written_at,
    }
}

fn idea(symbol: &str, conv: Option<Conviction>, entry: Option<&str>, inv: Option<&str>) -> RankedIdea {
    RankedIdea {
        symbol: symbol.to_string(),
        thesis_md: format!("thesis for {symbol}"),
        conviction: conv,
        entry_zone: entry.map(str::to_string),
        invalidation: inv.map(str::to_string),
        evidence_refs: vec![],
    }
}

#[tokio::test]
async fn record_predictions_inserts_one_row_per_idea() {
    let (_tmp, db) = open_db();
    let pack = pack_at(
        NaiveDate::from_ymd_opt(2026, 5, 4).unwrap(),
        Utc.timestamp_opt(1_714_780_800, 0).unwrap(),
        vec![
            idea("TSLA", Some(Conviction::A), Some("100-105"), Some("close < 95")),
            idea("aapl", Some(Conviction::B), Some("180"), None),
        ],
    );

    let written = record_predictions_from_pack(&db, &pack).await.expect("ok");
    assert_eq!(written.len(), 2);
    assert_eq!(written[0].symbol, "TSLA");
    assert_eq!(written[1].symbol, "AAPL");
    assert_eq!(written[0].source, SOURCE_AGENT_MORNING_SWEEP);
    assert_eq!(written[0].morning_pack_id.as_deref(), Some("2026-05-04"));
    assert_eq!(written[0].conviction, Some(Conviction::A));
    assert_eq!(written[0].entry_zone.as_deref(), Some("100-105"));
}

#[tokio::test]
async fn record_predictions_idempotent_replaces_existing() {
    let (_tmp, db) = open_db();
    let date = NaiveDate::from_ymd_opt(2026, 5, 5).unwrap();
    let written_at = Utc.timestamp_opt(1_714_867_200, 0).unwrap();

    let _ = record_predictions_from_pack(
        &db,
        &pack_at(
            date,
            written_at,
            vec![idea("TSLA", Some(Conviction::A), Some("100"), None)],
        ),
    )
    .await
    .unwrap();

    // Re-run with a different ideas list; old TSLA row must be gone.
    let _ = record_predictions_from_pack(
        &db,
        &pack_at(
            date,
            written_at,
            vec![idea("MSFT", Some(Conviction::C), Some("400"), None)],
        ),
    )
    .await
    .unwrap();

    let rows = list_predictions(&db, 0, None).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].symbol, "MSFT");
}

#[tokio::test]
async fn find_for_pack_returns_matching_prediction() {
    let (_tmp, db) = open_db();
    let date = NaiveDate::from_ymd_opt(2026, 5, 6).unwrap();
    let written_at = Utc.timestamp_opt(1_714_953_600, 0).unwrap();
    record_predictions_from_pack(
        &db,
        &pack_at(
            date,
            written_at,
            vec![
                idea("TSLA", Some(Conviction::A), Some("100-105"), Some("95")),
                idea("AAPL", Some(Conviction::B), Some("180"), None),
            ],
        ),
    )
    .await
    .unwrap();

    let row = find_for_pack(&db, "2026-05-06", "tsla")
        .await
        .unwrap()
        .expect("row present");
    assert_eq!(row.symbol, "TSLA");
    assert_eq!(row.conviction, Some(Conviction::A));

    let missing = find_for_pack(&db, "2026-05-06", "MISSING")
        .await
        .unwrap();
    assert!(missing.is_none());
}

#[tokio::test]
async fn list_predictions_filters_by_symbol_and_since() {
    let (_tmp, db) = open_db();

    record_predictions_from_pack(
        &db,
        &pack_at(
            NaiveDate::from_ymd_opt(2026, 5, 1).unwrap(),
            Utc.timestamp_opt(1_714_521_600, 0).unwrap(),
            vec![idea("TSLA", Some(Conviction::A), None, None)],
        ),
    )
    .await
    .unwrap();
    record_predictions_from_pack(
        &db,
        &pack_at(
            NaiveDate::from_ymd_opt(2026, 5, 4).unwrap(),
            Utc.timestamp_opt(1_714_780_800, 0).unwrap(),
            vec![
                idea("TSLA", Some(Conviction::B), None, None),
                idea("AAPL", Some(Conviction::C), None, None),
            ],
        ),
    )
    .await
    .unwrap();

    let recent = list_predictions(&db, 1_714_700_000, None).await.unwrap();
    assert_eq!(recent.len(), 2, "only the second pack's two rows are recent");

    let tsla_only = list_predictions(&db, 0, Some("tsla")).await.unwrap();
    assert_eq!(tsla_only.len(), 2);
    assert!(tsla_only.iter().all(|r| r.symbol == "TSLA"));
}
