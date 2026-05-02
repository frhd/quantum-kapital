use std::sync::Arc;

use chrono::Utc;
use tempfile::NamedTempFile;

use crate::services::research_notes::{
    self, Conviction, EvidenceRef, InvalidationKind, ListNotesQuery, NewResearchNote, NoteTarget,
    ResearchNotesError,
};
use crate::storage::Db;

fn open_db() -> (NamedTempFile, Arc<Db>) {
    let tmp = NamedTempFile::new().expect("tempfile");
    let db = Db::open(tmp.path()).expect("open db");
    (tmp, Arc::new(db))
}

fn sample(symbol: &str, written_by: &str) -> NewResearchNote {
    NewResearchNote {
        symbol: symbol.to_string(),
        body_md: "## Thesis\nlooks bullish".to_string(),
        conviction: Some(Conviction::B),
        evidence_refs: vec![],
        written_by: written_by.to_string(),
        setup_id: None,
        alert_id: None,
        price_at_write: None,
        invalidation_price: None,
        invalidation_kind: None,
        targets: vec![],
        catalyst_date: None,
    }
}

#[tokio::test]
async fn write_note_persists_row_with_normalized_symbol() {
    let (_tmp, db) = open_db();
    let saved = research_notes::write_note(&db, sample("tsla", "interactive"))
        .await
        .expect("write");
    assert_eq!(saved.symbol, "TSLA");
    assert_eq!(saved.body_md, "## Thesis\nlooks bullish");
    assert_eq!(saved.conviction, Some(Conviction::B));
    assert!(saved.id > 0);

    let fetched = research_notes::get_note(&db, saved.id)
        .await
        .expect("get")
        .expect("present");
    assert_eq!(fetched, saved);
}

#[tokio::test]
async fn write_note_rejects_blank_inputs() {
    let (_tmp, db) = open_db();
    let blank_symbol = NewResearchNote {
        symbol: "   ".to_string(),
        ..sample("AAPL", "interactive")
    };
    let err = research_notes::write_note(&db, blank_symbol)
        .await
        .expect_err("blank symbol must error");
    assert!(matches!(err, ResearchNotesError::EmptySymbol));

    let blank_body = NewResearchNote {
        body_md: "  \n  ".to_string(),
        ..sample("AAPL", "interactive")
    };
    let err = research_notes::write_note(&db, blank_body)
        .await
        .expect_err("blank body must error");
    assert!(matches!(err, ResearchNotesError::EmptyBody));

    let blank_caller = NewResearchNote {
        written_by: "".to_string(),
        ..sample("AAPL", "interactive")
    };
    let err = research_notes::write_note(&db, blank_caller)
        .await
        .expect_err("blank caller must error");
    assert!(matches!(err, ResearchNotesError::EmptyWrittenBy));
}

#[tokio::test]
async fn evidence_refs_round_trip_through_storage() {
    let (_tmp, db) = open_db();
    let from = Utc::now();
    let to = from + chrono::Duration::hours(2);
    let refs = vec![
        EvidenceRef::Alert { id: 7 },
        EvidenceRef::News { cache_id: 42 },
        EvidenceRef::Setup { id: 99 },
        EvidenceRef::BarRange {
            symbol: "TSLA".to_string(),
            from,
            to,
        },
    ];
    let saved = research_notes::write_note(
        &db,
        NewResearchNote {
            evidence_refs: refs.clone(),
            ..sample("TSLA", "agent_alert_dive")
        },
    )
    .await
    .expect("write");

    let fetched = research_notes::get_note(&db, saved.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.evidence_refs.len(), 4);
    // Each variant round-trips by exact value (serde tag = "type").
    assert_eq!(fetched.evidence_refs, refs);
}

#[tokio::test]
async fn structured_levels_round_trip_through_storage() {
    let (_tmp, db) = open_db();
    let saved = research_notes::write_note(
        &db,
        NewResearchNote {
            price_at_write: Some(166.48),
            invalidation_price: Some(156.0),
            invalidation_kind: Some(InvalidationKind::CloseBelow),
            targets: vec![
                NoteTarget {
                    label: "T1".to_string(),
                    price: 185.0,
                },
                NoteTarget {
                    label: "T2".to_string(),
                    price: 215.0,
                },
            ],
            catalyst_date: Some("2026-05-15".to_string()),
            ..sample("RDDT", "interactive")
        },
    )
    .await
    .expect("write");

    let fetched = research_notes::get_note(&db, saved.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.price_at_write, Some(166.48));
    assert_eq!(fetched.invalidation_price, Some(156.0));
    assert_eq!(
        fetched.invalidation_kind,
        Some(InvalidationKind::CloseBelow)
    );
    assert_eq!(fetched.targets.len(), 2);
    assert_eq!(fetched.targets[0].label, "T1");
    assert!((fetched.targets[0].price - 185.0).abs() < 1e-9);
    assert_eq!(fetched.catalyst_date.as_deref(), Some("2026-05-15"));
}

#[tokio::test]
async fn invalidation_kind_without_price_is_dropped() {
    // Half-set state would confuse the UI: the validity card relies on
    // `invalidation_price` to compute breach. If a caller hands us a
    // kind without a price, drop the kind so reads see a clean
    // "Unknown" status rather than "kind set, price null".
    let (_tmp, db) = open_db();
    let saved = research_notes::write_note(
        &db,
        NewResearchNote {
            invalidation_price: None,
            invalidation_kind: Some(InvalidationKind::CloseBelow),
            ..sample("AAPL", "interactive")
        },
    )
    .await
    .expect("write");
    assert_eq!(saved.invalidation_price, None);
    assert_eq!(saved.invalidation_kind, None);
    let fetched = research_notes::get_note(&db, saved.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.invalidation_kind, None);
}

#[tokio::test]
async fn list_notes_filters_by_symbol_and_orders_newest_first() {
    let (_tmp, db) = open_db();
    let _t = research_notes::write_note(&db, sample("TSLA", "interactive"))
        .await
        .unwrap();
    // Sleep 1s so written_at differs by at least one full second
    // (storage rounds to unix seconds — sub-second writes order by id
    // tiebreaker, but the ordering test should not rely on that).
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    let _a = research_notes::write_note(&db, sample("AAPL", "interactive"))
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    let _t2 = research_notes::write_note(&db, sample("TSLA", "agent_morning_sweep"))
        .await
        .unwrap();

    let all = research_notes::list_notes(
        &db,
        ListNotesQuery {
            limit: 50,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(all.len(), 3);
    // Newest first.
    assert_eq!(all[0].symbol, "TSLA");
    assert_eq!(all[0].written_by, "agent_morning_sweep");
    assert_eq!(all[2].symbol, "TSLA");
    assert_eq!(all[2].written_by, "interactive");

    let just_tsla = research_notes::list_notes(
        &db,
        ListNotesQuery {
            symbol: Some("tsla".to_string()),
            limit: 50,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(just_tsla.len(), 2);
    assert!(just_tsla.iter().all(|n| n.symbol == "TSLA"));
}

#[tokio::test]
async fn list_notes_filters_by_alert_id() {
    use crate::ibkr::types::tracker::{AlertKind, StrategyTag, TrackerSource};
    use crate::services::alerts::record_alert;
    use crate::services::tracker_service::TrackerService;
    use crate::strategies::{Direction, SetupCandidate, TargetLevel};

    let (_tmp, db) = open_db();
    // alert_id has a FK on alerts(id); seed real rows so the FK passes.
    let tracker = TrackerService::new(Arc::clone(&db));
    tracker
        .add("TSLA", TrackerSource::Manual, None, vec![], None)
        .await
        .unwrap();
    let candidate = SetupCandidate {
        strategy: "breakout",
        tag: StrategyTag::Breakout,
        direction: Direction::Long,
        conviction_signal: 0.7,
        trigger_price: 100.0,
        stop_price: 95.0,
        targets: vec![TargetLevel {
            label: "T1".to_string(),
            price: 110.0,
        }],
        raw_signals: serde_json::json!({}),
        timeframe: crate::ibkr::types::BarSize::Day1,
        detected_at: Utc::now(),
    };
    let setup = tracker.insert_setup("TSLA", &candidate).await.unwrap();
    let a1 = record_alert(&db, setup.id, AlertKind::Detected, serde_json::json!({}))
        .await
        .unwrap()
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    let a2 = record_alert(&db, setup.id, AlertKind::TargetHit, serde_json::json!({}))
        .await
        .unwrap()
        .unwrap();

    let _ = research_notes::write_note(
        &db,
        NewResearchNote {
            alert_id: Some(a1.id),
            ..sample("TSLA", "agent_alert_dive")
        },
    )
    .await
    .unwrap();
    let _ = research_notes::write_note(
        &db,
        NewResearchNote {
            alert_id: Some(a2.id),
            ..sample("TSLA", "agent_alert_dive")
        },
    )
    .await
    .unwrap();

    let only_a2 = research_notes::list_notes(
        &db,
        ListNotesQuery {
            alert_id: Some(a2.id),
            limit: 10,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(only_a2.len(), 1);
    assert_eq!(only_a2[0].alert_id, Some(a2.id));
}
