//! Phase 21 — Alert recording and feed queries.
//!
//! Thin SQL helpers around the `alerts` table. Producers call
//! [`record_alert`] inline at each emit site (one per [`AlertKind`]); the
//! frontend reads back through [`list_alerts`] / [`mark_alerts_seen`] via
//! the matching Tauri commands.
//!
//! Dedup is time-bounded: a second `record_alert` for the same
//! `(setup_id, kind)` within [`DEDUP_WINDOW`] is silently dropped, which
//! prevents the frontend feed from doubling up when an event is re-emitted
//! during a runner pass (e.g. Phase 17's thesis pipeline re-emits
//! `SetupDetected` after the LLM populates the thesis).

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use rusqlite::OptionalExtension;
use std::sync::Arc;

use crate::ibkr::types::tracker::{Alert, AlertKind};
use crate::storage::error::StorageError;
use crate::storage::Db;
use crate::utils::helpers::unix_to_utc;

mod ack;
pub use ack::{ack_alert, AckAlertError, AckAlertOutcome, AlertDecision};

#[cfg(test)]
mod tests;

/// Re-emit window. A second record for the same `(setup_id, kind)` pair
/// inside this window is suppressed. 1s is enough to absorb the runner's
/// "first pass + thesis-generated re-emit" flow without masking genuine
/// repeats minutes apart.
pub const DEDUP_WINDOW: ChronoDuration = ChronoDuration::seconds(1);

#[derive(Debug, Clone)]
pub struct ListAlertsQuery {
    pub limit: u32,
    pub offset: u32,
    pub since: Option<DateTime<Utc>>,
    pub kind: Option<AlertKind>,
    pub only_unseen: bool,
}

impl Default for ListAlertsQuery {
    fn default() -> Self {
        Self {
            limit: 50,
            offset: 0,
            since: None,
            kind: None,
            only_unseen: false,
        }
    }
}

/// Insert an alert row for `(setup_id, kind)` carrying `payload`. Returns
/// the persisted row, or `Ok(None)` when a recent duplicate (same
/// `setup_id` + `kind` within [`DEDUP_WINDOW`]) was already stored.
pub async fn record_alert(
    db: &Arc<Db>,
    setup_id: i64,
    kind: AlertKind,
    payload: serde_json::Value,
) -> Result<Option<Alert>, StorageError> {
    let now = Utc::now();
    let now_unix = now.timestamp();
    let dedup_cutoff = (now - DEDUP_WINDOW).timestamp();
    let kind_str = kind.as_str().to_string();
    let payload_str = serde_json::to_string(&payload).map_err(StorageError::from)?;

    let kind_for_db = kind_str.clone();
    let payload_for_db = payload_str.clone();
    let inserted_id = db
        .with_conn(move |conn| {
            // Suppress same (setup_id, kind) within the dedup window.
            let recent: Option<i64> = conn
                .query_row(
                    "SELECT id FROM alerts \
                     WHERE setup_id = ?1 AND kind = ?2 AND fired_at >= ?3 \
                     ORDER BY fired_at DESC LIMIT 1",
                    rusqlite::params![setup_id, kind_for_db, dedup_cutoff],
                    |row| row.get::<_, i64>(0),
                )
                .optional()
                .map_err(StorageError::from)?;
            if recent.is_some() {
                return Ok(None);
            }
            conn.execute(
                "INSERT INTO alerts (setup_id, kind, fired_at, payload, seen) \
                 VALUES (?1, ?2, ?3, ?4, 0)",
                rusqlite::params![setup_id, kind_for_db, now_unix, payload_for_db],
            )?;
            Ok(Some(conn.last_insert_rowid()))
        })
        .await?;

    let Some(id) = inserted_id else {
        return Ok(None);
    };

    Ok(Some(Alert {
        id,
        setup_id,
        kind,
        fired_at: unix_to_utc(now_unix),
        payload,
        seen: false,
    }))
}

/// Read a slice of the feed. Rows come back newest-first
/// (`fired_at DESC, id DESC`). All filters are AND-combined.
pub async fn list_alerts(db: &Arc<Db>, query: ListAlertsQuery) -> Result<Vec<Alert>, StorageError> {
    let limit = query.limit.max(1) as i64;
    let offset = query.offset as i64;
    let since_unix = query.since.map(|d| d.timestamp());
    let kind_str = query.kind.map(|k| k.as_str().to_string());
    let only_unseen = query.only_unseen;

    let raws = db
        .with_conn(move |conn| {
            let mut sql =
                String::from("SELECT id, setup_id, kind, fired_at, payload, seen FROM alerts");
            let mut clauses: Vec<String> = Vec::new();
            let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
            if let Some(ts) = since_unix {
                clauses.push(format!("fired_at >= ?{}", params.len() + 1));
                params.push(Box::new(ts));
            }
            if let Some(k) = kind_str {
                clauses.push(format!("kind = ?{}", params.len() + 1));
                params.push(Box::new(k));
            }
            if only_unseen {
                clauses.push("seen = 0".to_string());
            }
            if !clauses.is_empty() {
                sql.push_str(" WHERE ");
                sql.push_str(&clauses.join(" AND "));
            }
            sql.push_str(" ORDER BY fired_at DESC, id DESC");
            sql.push_str(&format!(
                " LIMIT ?{} OFFSET ?{}",
                params.len() + 1,
                params.len() + 2
            ));
            params.push(Box::new(limit));
            params.push(Box::new(offset));

            let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt
                .query_map(param_refs.as_slice(), row_to_raw)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .await?;

    raws.into_iter().map(decode_raw).collect()
}

/// Mark every alert whose id appears in `ids` as `seen=1`. Unknown ids are
/// silently skipped (no error). Returns the number of rows actually
/// flipped.
pub async fn mark_alerts_seen(db: &Arc<Db>, ids: Vec<i64>) -> Result<usize, StorageError> {
    if ids.is_empty() {
        return Ok(0);
    }
    let updated = db
        .with_conn(move |conn| {
            let placeholders = std::iter::repeat_n("?", ids.len())
                .collect::<Vec<_>>()
                .join(",");
            let sql =
                format!("UPDATE alerts SET seen = 1 WHERE seen = 0 AND id IN ({placeholders})");
            let params: Vec<&dyn rusqlite::ToSql> =
                ids.iter().map(|id| id as &dyn rusqlite::ToSql).collect();
            let n = conn.execute(&sql, params.as_slice())?;
            Ok(n)
        })
        .await?;
    Ok(updated)
}

// ---------------- internals ----------------

type RawRow = (
    i64,    // id
    i64,    // setup_id
    String, // kind
    i64,    // fired_at unix
    String, // payload json
    i64,    // seen 0/1
);

fn row_to_raw(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawRow> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
    ))
}

fn decode_raw(r: RawRow) -> Result<Alert, StorageError> {
    let (id, setup_id, kind_s, fired_at, payload_s, seen) = r;
    let kind = AlertKind::parse(&kind_s).ok_or_else(|| {
        StorageError::Migration(format!("unknown alert kind '{kind_s}' on alert#{id}"))
    })?;
    let payload: serde_json::Value =
        serde_json::from_str(&payload_s).map_err(StorageError::from)?;
    Ok(Alert {
        id,
        setup_id,
        kind,
        fired_at: unix_to_utc(fired_at),
        payload,
        seen: seen != 0,
    })
}
