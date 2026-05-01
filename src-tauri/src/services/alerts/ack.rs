//! Phase 02 — `ack_alert` decision rail.
//!
//! Closes the loop on an alert by recording the user / agent's decision
//! (acted / passed / researching). When a `note` is supplied, the rail
//! also persists a [`crate::services::research_notes::ResearchNote`]
//! linked back to the alert so the eval harness can later compare
//! decisions against subsequent setup outcomes.
//!
//! Stored fields: `alerts.decision`, `alerts.decision_note_id`,
//! `alerts.decided_at` — added by V03. The note row stamps `written_by`
//! with the same `caller` string the audit log records, so an audit row
//! and a note row can be cross-referenced without ambiguity.

use std::sync::Arc;

use chrono::Utc;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::services::research_notes::{
    self, NewResearchNote, ResearchNote, ResearchNotesError,
};
use crate::storage::error::StorageError;
use crate::storage::Db;

/// Closed set of decisions a user / agent can record against an alert.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertDecision {
    Acted,
    Passed,
    Researching,
}

impl AlertDecision {
    pub fn as_str(&self) -> &'static str {
        match self {
            AlertDecision::Acted => "acted",
            AlertDecision::Passed => "passed",
            AlertDecision::Researching => "researching",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "acted" => Some(AlertDecision::Acted),
            "passed" => Some(AlertDecision::Passed),
            "researching" => Some(AlertDecision::Researching),
            _ => None,
        }
    }
}

#[derive(Error, Debug)]
pub enum AckAlertError {
    #[error("alert#{0} not found")]
    AlertNotFound(i64),
    #[error("setup#{0} not found for alert")]
    SetupNotFound(i64),
    #[error("caller must be non-empty")]
    EmptyCaller,
    #[error("storage: {0}")]
    Storage(#[from] StorageError),
    #[error("research note: {0}")]
    Notes(#[from] ResearchNotesError),
}

/// Result of [`ack_alert`]. `note` is `Some(...)` exactly when the
/// caller supplied a non-blank `note`.
#[derive(Debug, Clone)]
pub struct AckAlertOutcome {
    pub alert_id: i64,
    pub decision: AlertDecision,
    pub note: Option<ResearchNote>,
}

/// Record a decision against an alert. When `note` is non-blank, also
/// creates a research_note linked to the alert and stores its id back
/// on `alerts.decision_note_id`.
pub async fn ack_alert(
    db: &Arc<Db>,
    alert_id: i64,
    decision: AlertDecision,
    note: Option<&str>,
    caller: &str,
) -> Result<AckAlertOutcome, AckAlertError> {
    if caller.trim().is_empty() {
        return Err(AckAlertError::EmptyCaller);
    }

    // Resolve the alert + its setup so the (optional) note row carries the
    // correct symbol.
    let setup_id = lookup_setup_id(db, alert_id).await?;
    let symbol = lookup_setup_symbol(db, setup_id).await?;

    // Persist the optional note first so we can stash its id on the alert
    // in the same row update.
    let note_row = match note.and_then(|s| {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(s.to_string())
        }
    }) {
        Some(body) => {
            let saved = research_notes::write_note(
                db,
                NewResearchNote {
                    symbol,
                    body_md: body,
                    conviction: None,
                    evidence_refs: vec![],
                    written_by: caller.to_string(),
                    setup_id: Some(setup_id),
                    alert_id: Some(alert_id),
                },
            )
            .await?;
            Some(saved)
        }
        None => None,
    };

    let now_unix = Utc::now().timestamp();
    let decision_str = decision.as_str().to_string();
    let note_id = note_row.as_ref().map(|n| n.id);

    let updated = db
        .with_conn(move |conn| {
            let n = conn.execute(
                "UPDATE alerts SET decision = ?1, decision_note_id = ?2, decided_at = ?3 \
                 WHERE id = ?4",
                rusqlite::params![decision_str, note_id, now_unix, alert_id],
            )?;
            Ok(n)
        })
        .await?;
    if updated == 0 {
        // Alert vanished mid-flight (deleted between lookup and update).
        return Err(AckAlertError::AlertNotFound(alert_id));
    }

    Ok(AckAlertOutcome {
        alert_id,
        decision,
        note: note_row,
    })
}

async fn lookup_setup_id(db: &Arc<Db>, alert_id: i64) -> Result<i64, AckAlertError> {
    let setup_id = db
        .with_conn(move |conn| {
            conn.query_row(
                "SELECT setup_id FROM alerts WHERE id = ?1",
                rusqlite::params![alert_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(StorageError::from)
        })
        .await?;
    setup_id.ok_or(AckAlertError::AlertNotFound(alert_id))
}

async fn lookup_setup_symbol(db: &Arc<Db>, setup_id: i64) -> Result<String, AckAlertError> {
    let symbol = db
        .with_conn(move |conn| {
            conn.query_row(
                "SELECT symbol FROM setups WHERE id = ?1",
                rusqlite::params![setup_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(StorageError::from)
        })
        .await?;
    symbol.ok_or(AckAlertError::SetupNotFound(setup_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    use chrono::Utc;
    use serde_json::json;
    use tempfile::NamedTempFile;

    use crate::ibkr::types::tracker::{AlertKind, StrategyTag, TrackerSource};
    use crate::services::alerts::record_alert;
    use crate::services::research_notes::list_notes;
    use crate::services::research_notes::ListNotesQuery;
    use crate::services::tracker_service::TrackerService;
    use crate::strategies::{Direction, SetupCandidate, TargetLevel};

    async fn seed(symbol: &str) -> (NamedTempFile, Arc<Db>, i64) {
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
        (tmp, db, alert.id)
    }

    #[tokio::test]
    async fn ack_alert_with_note_creates_linked_research_note() {
        let (_tmp, db, alert_id) = seed("AAPL").await;

        let out = ack_alert(
            &db,
            alert_id,
            AlertDecision::Acted,
            Some("opened starter long, sized 0.5R"),
            "interactive",
        )
        .await
        .expect("ack ok");

        assert_eq!(out.alert_id, alert_id);
        assert_eq!(out.decision, AlertDecision::Acted);
        let note = out.note.expect("note created");
        assert_eq!(note.alert_id, Some(alert_id));
        assert_eq!(note.symbol, "AAPL");
        assert_eq!(note.written_by, "interactive");

        // Note is reachable through list_notes by alert_id.
        let listed = list_notes(
            &db,
            ListNotesQuery {
                alert_id: Some(alert_id),
                limit: 10,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, note.id);

        // alerts.decision_note_id stamped to point at the note.
        let stored: (Option<String>, Option<i64>, Option<i64>) = db
            .with_conn(move |conn| {
                conn.query_row(
                    "SELECT decision, decision_note_id, decided_at FROM alerts WHERE id = ?1",
                    rusqlite::params![alert_id],
                    |row| {
                        Ok((
                            row.get::<_, Option<String>>(0)?,
                            row.get::<_, Option<i64>>(1)?,
                            row.get::<_, Option<i64>>(2)?,
                        ))
                    },
                )
                .map_err(StorageError::from)
            })
            .await
            .unwrap();
        assert_eq!(stored.0.as_deref(), Some("acted"));
        assert_eq!(stored.1, Some(note.id));
        assert!(stored.2.is_some());
    }

    #[tokio::test]
    async fn ack_alert_without_note_only_records_decision() {
        let (_tmp, db, alert_id) = seed("TSLA").await;

        let out = ack_alert(&db, alert_id, AlertDecision::Passed, None, "interactive")
            .await
            .expect("ack ok");
        assert!(out.note.is_none());

        let stored: (Option<String>, Option<i64>) = db
            .with_conn(move |conn| {
                conn.query_row(
                    "SELECT decision, decision_note_id FROM alerts WHERE id = ?1",
                    rusqlite::params![alert_id],
                    |row| {
                        Ok((
                            row.get::<_, Option<String>>(0)?,
                            row.get::<_, Option<i64>>(1)?,
                        ))
                    },
                )
                .map_err(StorageError::from)
            })
            .await
            .unwrap();
        assert_eq!(stored.0.as_deref(), Some("passed"));
        assert_eq!(stored.1, None);

        // No research_note row was created.
        let listed = list_notes(
            &db,
            ListNotesQuery {
                alert_id: Some(alert_id),
                limit: 10,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(listed.is_empty());
    }

    #[tokio::test]
    async fn ack_alert_blank_note_skips_research_note_creation() {
        let (_tmp, db, alert_id) = seed("MSFT").await;
        let out = ack_alert(
            &db,
            alert_id,
            AlertDecision::Researching,
            Some("   \n  "),
            "agent_alert_dive",
        )
        .await
        .expect("ack ok");
        assert!(out.note.is_none(), "blank note must not create a row");
    }

    #[tokio::test]
    async fn ack_alert_unknown_alert_errors() {
        let (_tmp, db, _) = seed("AAPL").await;
        let err = ack_alert(&db, 99_999, AlertDecision::Acted, None, "interactive")
            .await
            .expect_err("unknown alert");
        match err {
            AckAlertError::AlertNotFound(id) => assert_eq!(id, 99_999),
            other => panic!("expected AlertNotFound, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn ack_alert_blank_caller_errors() {
        let (_tmp, db, alert_id) = seed("AAPL").await;
        let err = ack_alert(&db, alert_id, AlertDecision::Acted, None, "  ")
            .await
            .expect_err("blank caller");
        assert!(matches!(err, AckAlertError::EmptyCaller));
    }
}
