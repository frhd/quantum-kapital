//! Phase 6 — `mark_alert_enriched` rail.
//!
//! Closes the loop on the per-alert deep-dive agent: stamps
//! `alerts.enriched_at` (and optionally `alerts.research_note_id`) so the
//! same alert is never enriched twice. Idempotent — re-calling for an
//! already-enriched alert is a no-op that returns the existing values
//! instead of erroring.
//!
//! `research_note_id` is optional so the agent can record a "skipped"
//! enrichment (e.g. global budget exhausted) without manufacturing a
//! note.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::storage::error::StorageError;
use crate::storage::Db;
use crate::utils::helpers::unix_to_utc;

#[derive(Error, Debug)]
pub enum MarkEnrichedError {
    #[error("alert#{0} not found")]
    AlertNotFound(i64),
    #[error("research_note#{0} not found")]
    ResearchNoteNotFound(i64),
    #[error("storage: {0}")]
    Storage(#[from] StorageError),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarkEnrichedOutcome {
    pub alert_id: i64,
    pub enriched_at: DateTime<Utc>,
    pub research_note_id: Option<i64>,
    /// `true` when this call performed the marking, `false` when the
    /// alert was already enriched and we returned the existing values.
    pub newly_marked: bool,
}

/// Stamp `alerts.enriched_at = now` and, when supplied,
/// `alerts.research_note_id`. Idempotent: a second call with the same
/// `alert_id` returns the existing stamp without modifying it (the
/// `research_note_id` argument on the second call is ignored).
pub async fn mark_alert_enriched(
    db: &Arc<Db>,
    alert_id: i64,
    research_note_id: Option<i64>,
) -> Result<MarkEnrichedOutcome, MarkEnrichedError> {
    if let Some(note_id) = research_note_id {
        let exists = db
            .with_conn(move |conn| {
                conn.query_row(
                    "SELECT 1 FROM research_notes WHERE id = ?1",
                    rusqlite::params![note_id],
                    |_| Ok(()),
                )
                .optional()
                .map_err(StorageError::from)
            })
            .await?;
        if exists.is_none() {
            return Err(MarkEnrichedError::ResearchNoteNotFound(note_id));
        }
    }

    let existing = db
        .with_conn(move |conn| {
            conn.query_row(
                "SELECT enriched_at, research_note_id FROM alerts WHERE id = ?1",
                rusqlite::params![alert_id],
                |row| Ok((row.get::<_, Option<i64>>(0)?, row.get::<_, Option<i64>>(1)?)),
            )
            .optional()
            .map_err(StorageError::from)
        })
        .await?;

    let Some((existing_enriched, existing_note)) = existing else {
        return Err(MarkEnrichedError::AlertNotFound(alert_id));
    };

    if let Some(ts) = existing_enriched {
        return Ok(MarkEnrichedOutcome {
            alert_id,
            enriched_at: unix_to_utc(ts),
            research_note_id: existing_note,
            newly_marked: false,
        });
    }

    let now = Utc::now();
    let now_unix = now.timestamp();
    let updated = db
        .with_conn(move |conn| {
            let n = conn.execute(
                "UPDATE alerts SET enriched_at = ?1, research_note_id = ?2 \
                 WHERE id = ?3 AND enriched_at IS NULL",
                rusqlite::params![now_unix, research_note_id, alert_id],
            )?;
            Ok(n)
        })
        .await?;

    if updated == 0 {
        // Race: another writer enriched between our SELECT and UPDATE.
        // Re-read and treat as not-newly-marked.
        let (ts, note) = db
            .with_conn(move |conn| {
                conn.query_row(
                    "SELECT enriched_at, research_note_id FROM alerts WHERE id = ?1",
                    rusqlite::params![alert_id],
                    |row| Ok((row.get::<_, Option<i64>>(0)?, row.get::<_, Option<i64>>(1)?)),
                )
                .optional()
                .map_err(StorageError::from)
            })
            .await?
            .ok_or(MarkEnrichedError::AlertNotFound(alert_id))?;
        return Ok(MarkEnrichedOutcome {
            alert_id,
            enriched_at: ts.map(unix_to_utc).unwrap_or_else(|| unix_to_utc(now_unix)),
            research_note_id: note,
            newly_marked: false,
        });
    }

    Ok(MarkEnrichedOutcome {
        alert_id,
        // Truncate to second resolution so a follow-up read sees the
        // same value the database persisted (rows are unix-second).
        enriched_at: unix_to_utc(now_unix),
        research_note_id,
        newly_marked: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use serde_json::json;
    use tempfile::NamedTempFile;

    use crate::ibkr::types::tracker::{AlertKind, StrategyTag, TrackerSource};
    use crate::services::alerts::record_alert;
    use crate::services::research_notes::{self, NewResearchNote};
    use crate::services::tracker_service::TrackerService;
    use crate::strategies::{Direction, SetupCandidate, TargetLevel};

    async fn seed_alert_and_note(symbol: &str) -> (NamedTempFile, Arc<Db>, i64, i64) {
        let tmp = NamedTempFile::new().expect("tempfile");
        let db = Arc::new(Db::open(tmp.path()).expect("open db"));
        let svc = TrackerService::new(Arc::clone(&db));
        svc.add(symbol, TrackerSource::Manual, None, vec![], None)
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
            raw_signals: json!({}),
            timeframe: crate::ibkr::types::historical::BarSize::Day1,
            detected_at: Utc::now(),
        };
        let setup = svc.insert_setup(symbol, &candidate).await.unwrap();
        let alert = record_alert(&db, setup.id, AlertKind::Detected, json!({"x": 1}))
            .await
            .unwrap()
            .unwrap();
        let note = research_notes::write_note(
            &db,
            NewResearchNote {
                symbol: symbol.to_string(),
                body_md: "deep dive".to_string(),
                conviction: None,
                evidence_refs: vec![],
                written_by: "agent_alert_dive".to_string(),
                setup_id: Some(setup.id),
                alert_id: Some(alert.id),
                price_at_write: None,
                invalidation_price: None,
                invalidation_kind: None,
                targets: vec![],
                catalyst_date: None,
            },
        )
        .await
        .unwrap();
        (tmp, db, alert.id, note.id)
    }

    #[tokio::test]
    async fn first_call_marks_enriched() {
        let (_tmp, db, alert_id, note_id) = seed_alert_and_note("AAPL").await;
        let out = mark_alert_enriched(&db, alert_id, Some(note_id))
            .await
            .expect("ok");
        assert!(out.newly_marked);
        assert_eq!(out.alert_id, alert_id);
        assert_eq!(out.research_note_id, Some(note_id));
    }

    #[tokio::test]
    async fn second_call_is_idempotent_noop() {
        let (_tmp, db, alert_id, note_id) = seed_alert_and_note("MSFT").await;
        let first = mark_alert_enriched(&db, alert_id, Some(note_id))
            .await
            .expect("ok");
        // Second call carries a different note id — must be ignored.
        let second = mark_alert_enriched(&db, alert_id, None).await.expect("ok");
        assert!(first.newly_marked);
        assert!(!second.newly_marked);
        assert_eq!(second.research_note_id, Some(note_id));
        assert_eq!(second.enriched_at, first.enriched_at);
    }

    #[tokio::test]
    async fn skip_record_persists_without_note() {
        let (_tmp, db, alert_id, _) = seed_alert_and_note("TSLA").await;
        let out = mark_alert_enriched(&db, alert_id, None).await.expect("ok");
        assert!(out.newly_marked);
        assert!(out.research_note_id.is_none());
    }

    #[tokio::test]
    async fn unknown_alert_errors() {
        let (_tmp, db, _, note_id) = seed_alert_and_note("AAPL").await;
        let err = mark_alert_enriched(&db, 99_999, Some(note_id))
            .await
            .expect_err("unknown alert");
        assert!(matches!(err, MarkEnrichedError::AlertNotFound(99_999)));
    }

    #[tokio::test]
    async fn unknown_note_errors_before_touching_alert() {
        let (_tmp, db, alert_id, _) = seed_alert_and_note("AAPL").await;
        let err = mark_alert_enriched(&db, alert_id, Some(424_242))
            .await
            .expect_err("unknown note");
        assert!(matches!(
            err,
            MarkEnrichedError::ResearchNoteNotFound(424_242)
        ));
    }
}
