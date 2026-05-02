use std::sync::Arc;

use chrono::NaiveDate;
use tempfile::NamedTempFile;

use crate::services::agent_morning_packs::{
    self, AgentMorningPackError, NewAgentMorningPack, RankedIdea,
};
use crate::services::research_notes::Conviction;
use crate::storage::Db;

fn open_db() -> (NamedTempFile, Arc<Db>) {
    let tmp = NamedTempFile::new().expect("tempfile");
    let db = Db::open(tmp.path()).expect("open db");
    (tmp, Arc::new(db))
}

fn idea(symbol: &str, thesis: &str) -> RankedIdea {
    RankedIdea {
        symbol: symbol.to_string(),
        thesis_md: thesis.to_string(),
        conviction: Some(Conviction::A),
        entry_zone: Some("105-107".to_string()),
        invalidation: Some("close < 100".to_string()),
        evidence_refs: vec![],
    }
}

fn date(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

#[tokio::test]
async fn write_pack_persists_and_normalizes_symbols() {
    let (_tmp, db) = open_db();
    let saved = agent_morning_packs::write_pack(
        &db,
        NewAgentMorningPack {
            date: date("2026-05-04"),
            ranked_ideas: vec![idea("tsla", "...")],
            written_by: "agent_morning_sweep".to_string(),
        },
    )
    .await
    .expect("write");
    assert_eq!(saved.ranked_ideas.len(), 1);
    assert_eq!(saved.ranked_ideas[0].symbol, "TSLA");

    let fetched = agent_morning_packs::get_pack(&db, date("2026-05-04"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.date, date("2026-05-04"));
    assert_eq!(fetched.ranked_ideas[0].symbol, "TSLA");
    assert_eq!(fetched.written_by, "agent_morning_sweep");
}

#[tokio::test]
async fn write_pack_is_idempotent_on_date() {
    // Master plan exit criterion: a second `write_morning_pack(date, ...)`
    // overwrites cleanly with no duplicate rows.
    let (_tmp, db) = open_db();
    let _ = agent_morning_packs::write_pack(
        &db,
        NewAgentMorningPack {
            date: date("2026-05-05"),
            ranked_ideas: vec![idea("AAPL", "first")],
            written_by: "agent_morning_sweep".to_string(),
        },
    )
    .await
    .unwrap();
    let _ = agent_morning_packs::write_pack(
        &db,
        NewAgentMorningPack {
            date: date("2026-05-05"),
            ranked_ideas: vec![idea("MSFT", "second"), idea("NVDA", "second-2")],
            written_by: "agent_morning_sweep".to_string(),
        },
    )
    .await
    .unwrap();

    let fetched = agent_morning_packs::get_pack(&db, date("2026-05-05"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.ranked_ideas.len(), 2);
    let symbols: Vec<&str> = fetched
        .ranked_ideas
        .iter()
        .map(|i| i.symbol.as_str())
        .collect();
    assert_eq!(symbols, vec!["MSFT", "NVDA"]);

    // No duplicate rows in storage.
    let row_count: i64 = db
        .with_conn(|conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM agent_morning_packs WHERE date = '2026-05-05'",
                [],
                |row| row.get(0),
            )
            .map_err(crate::storage::StorageError::from)
        })
        .await
        .unwrap();
    assert_eq!(row_count, 1);
}

#[tokio::test]
async fn write_pack_rejects_blank_inputs() {
    let (_tmp, db) = open_db();
    let err = agent_morning_packs::write_pack(
        &db,
        NewAgentMorningPack {
            date: date("2026-05-06"),
            ranked_ideas: vec![idea("AAPL", "x")],
            written_by: "".to_string(),
        },
    )
    .await
    .expect_err("blank caller");
    assert!(matches!(err, AgentMorningPackError::EmptyWrittenBy));

    let err = agent_morning_packs::write_pack(
        &db,
        NewAgentMorningPack {
            date: date("2026-05-06"),
            ranked_ideas: vec![],
            written_by: "agent_morning_sweep".to_string(),
        },
    )
    .await
    .expect_err("empty ideas");
    assert!(matches!(err, AgentMorningPackError::EmptyIdeas));
}

#[tokio::test]
async fn write_pack_snapshots_predictions() {
    // Phase 8: each ranked idea must produce a `predictions` row,
    // and a re-write of the same date replaces the prior set.
    let (_tmp, db) = open_db();
    agent_morning_packs::write_pack(
        &db,
        NewAgentMorningPack {
            date: date("2026-05-07"),
            ranked_ideas: vec![idea("TSLA", "first"), idea("AAPL", "second")],
            written_by: "agent_morning_sweep".to_string(),
        },
    )
    .await
    .unwrap();

    let preds = crate::services::predictions::list_predictions(&db, 0, None)
        .await
        .unwrap();
    assert_eq!(preds.len(), 2);
    let mut symbols: Vec<&str> = preds.iter().map(|p| p.symbol.as_str()).collect();
    symbols.sort();
    assert_eq!(symbols, vec!["AAPL", "TSLA"]);
    assert!(preds
        .iter()
        .all(|p| p.morning_pack_id.as_deref() == Some("2026-05-07")));

    // Re-write replaces the snapshot set.
    agent_morning_packs::write_pack(
        &db,
        NewAgentMorningPack {
            date: date("2026-05-07"),
            ranked_ideas: vec![idea("MSFT", "rerun")],
            written_by: "agent_morning_sweep".to_string(),
        },
    )
    .await
    .unwrap();
    let preds = crate::services::predictions::list_predictions(&db, 0, None)
        .await
        .unwrap();
    assert_eq!(preds.len(), 1);
    assert_eq!(preds[0].symbol, "MSFT");
}

#[tokio::test]
async fn list_packs_orders_newest_first() {
    let (_tmp, db) = open_db();
    for d in ["2026-05-01", "2026-05-03", "2026-05-02"] {
        agent_morning_packs::write_pack(
            &db,
            NewAgentMorningPack {
                date: date(d),
                ranked_ideas: vec![idea("AAPL", "x")],
                written_by: "agent_morning_sweep".to_string(),
            },
        )
        .await
        .unwrap();
    }
    let packs = agent_morning_packs::list_packs(&db, 10).await.unwrap();
    let dates: Vec<String> = packs.iter().map(|p| p.date.to_string()).collect();
    assert_eq!(
        dates,
        vec![
            "2026-05-03".to_string(),
            "2026-05-02".to_string(),
            "2026-05-01".to_string()
        ]
    );
}
