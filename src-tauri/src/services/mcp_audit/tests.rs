use std::sync::Arc;

use serde_json::json;

use crate::services::mcp_audit;
use crate::storage::Db;
use tempfile::NamedTempFile;

fn open_db() -> (NamedTempFile, Arc<Db>) {
    let tmp = NamedTempFile::new().expect("tempfile");
    let db = Db::open(tmp.path()).expect("open db");
    (tmp, Arc::new(db))
}

#[tokio::test]
async fn record_persists_row_and_returns_monotonic_id() {
    let (_tmp, db) = open_db();

    let id1 = mcp_audit::record(
        &db,
        "write_research_note",
        &json!({"symbol": "AAPL", "body_md": "first"}),
        Some("research_notes.id=1"),
        Some("agent_morning_sweep"),
    )
    .await
    .expect("record 1");
    let id2 = mcp_audit::record(
        &db,
        "write_morning_pack",
        &json!({"date": "2026-05-01"}),
        None,
        Some("interactive"),
    )
    .await
    .expect("record 2");

    assert!(id2 > id1, "ids must be monotonic; got {id1}, {id2}");

    let rows = mcp_audit::list(&db, 50, 0).await.expect("list");
    assert_eq!(rows.len(), 2);
    // newest-first.
    assert_eq!(rows[0].tool, "write_morning_pack");
    assert_eq!(rows[0].caller.as_deref(), Some("interactive"));
    assert_eq!(rows[1].tool, "write_research_note");
    assert_eq!(
        rows[1].result_summary.as_deref(),
        Some("research_notes.id=1")
    );
    assert_eq!(rows[1].input["symbol"], "AAPL");
}

#[tokio::test]
async fn list_respects_limit_and_offset() {
    let (_tmp, db) = open_db();

    for i in 0..5 {
        mcp_audit::record(
            &db,
            "write_research_note",
            &json!({"i": i}),
            None,
            Some("interactive"),
        )
        .await
        .expect("record");
    }

    let first_two = mcp_audit::list(&db, 2, 0).await.expect("list");
    assert_eq!(first_two.len(), 2);
    let next_two = mcp_audit::list(&db, 2, 2).await.expect("list");
    assert_eq!(next_two.len(), 2);
    // First two should be newest (highest i); next two should follow.
    let first_ids: Vec<i64> = first_two.iter().map(|r| r.id).collect();
    let next_ids: Vec<i64> = next_two.iter().map(|r| r.id).collect();
    assert!(
        first_ids.iter().min() > next_ids.iter().max(),
        "pagination must order newest-first: {first_ids:?} vs {next_ids:?}"
    );
}
