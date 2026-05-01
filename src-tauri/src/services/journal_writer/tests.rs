//! Unit tests for the journal_writer service.

use chrono::NaiveDate;

use super::*;
use crate::mcp::tools::test_support::make_db;

fn date(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

#[tokio::test]
async fn upsert_entry_inserts_then_overwrites_on_date_section_key() {
    let (_tmp, db) = make_db();
    let d = date("2026-05-02");

    let first = upsert_entry(
        &db,
        NewJournalEntry {
            journal_date: d,
            section: "EOD Review (Agent)".into(),
            body_md: "first body".into(),
            written_by: "agent_eod_review".into(),
        },
    )
    .await
    .unwrap();
    assert!(first.id > 0);
    assert_eq!(first.body_md, "first body");

    // Same key — must upsert in place.
    let second = upsert_entry(
        &db,
        NewJournalEntry {
            journal_date: d,
            section: "EOD Review (Agent)".into(),
            body_md: "second body".into(),
            written_by: "agent_eod_review".into(),
        },
    )
    .await
    .unwrap();
    assert_eq!(second.id, first.id, "must upsert, not insert");
    assert_eq!(second.body_md, "second body");

    // Distinct section — separate row.
    let other = upsert_entry(
        &db,
        NewJournalEntry {
            journal_date: d,
            section: "Notes".into(),
            body_md: "user note".into(),
            written_by: "user".into(),
        },
    )
    .await
    .unwrap();
    assert_ne!(other.id, first.id);

    let entries = list_entries_for_date(&db, d).await.unwrap();
    assert_eq!(entries.len(), 2);
}

#[tokio::test]
async fn upsert_entry_rejects_empty_inputs() {
    let (_tmp, db) = make_db();
    let d = date("2026-05-02");

    let r = upsert_entry(
        &db,
        NewJournalEntry {
            journal_date: d,
            section: "  ".into(),
            body_md: "x".into(),
            written_by: "agent".into(),
        },
    )
    .await;
    assert!(matches!(r, Err(JournalWriterError::EmptySection)));

    let r = upsert_entry(
        &db,
        NewJournalEntry {
            journal_date: d,
            section: "S".into(),
            body_md: "  \n  ".into(),
            written_by: "agent".into(),
        },
    )
    .await;
    assert!(matches!(r, Err(JournalWriterError::EmptyBody)));

    let r = upsert_entry(
        &db,
        NewJournalEntry {
            journal_date: d,
            section: "S".into(),
            body_md: "x".into(),
            written_by: "  ".into(),
        },
    )
    .await;
    assert!(matches!(r, Err(JournalWriterError::EmptyWrittenBy)));
}

#[tokio::test]
async fn get_entry_returns_none_when_absent() {
    let (_tmp, db) = make_db();
    let r = get_entry(&db, date("2026-05-02"), "missing").await.unwrap();
    assert!(r.is_none());
}

#[tokio::test]
async fn list_entries_for_date_filters_by_date() {
    let (_tmp, db) = make_db();
    let d1 = date("2026-05-01");
    let d2 = date("2026-05-02");
    upsert_entry(
        &db,
        NewJournalEntry {
            journal_date: d1,
            section: "S".into(),
            body_md: "a".into(),
            written_by: "u".into(),
        },
    )
    .await
    .unwrap();
    upsert_entry(
        &db,
        NewJournalEntry {
            journal_date: d2,
            section: "S".into(),
            body_md: "b".into(),
            written_by: "u".into(),
        },
    )
    .await
    .unwrap();

    let only_d2 = list_entries_for_date(&db, d2).await.unwrap();
    assert_eq!(only_d2.len(), 1);
    assert_eq!(only_d2[0].journal_date, d2);
}
