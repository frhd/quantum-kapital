//! Phase 3 — `BracketGroupStore`: persistence for `bracket_groups`.
//!
//! Mirrors the same shape as `OrderIntentStore`: all writes go through
//! a single async wrapper over the shared `Db` handle. The service
//! layer (`OrderTicket`) is the only thing that talks to this store;
//! Tauri commands query through `OrderTicket::status` /
//! `OrderTicket::cancel`.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use rusqlite::{params, OptionalExtension};

use crate::storage::error::Result as StorageResult;
use crate::storage::Db;

use super::types::{BracketGroupRecord, BracketStatus, TargetSpec};

#[derive(Clone)]
pub struct BracketGroupStore {
    db: Arc<Db>,
}

impl BracketGroupStore {
    pub fn new(db: Arc<Db>) -> Self {
        Self { db }
    }

    /// Insert a new bracket-group row. Idempotent on `parent_order_id`
    /// (a re-call from a retried Tauri command lands a no-op rather
    /// than an error). Returns the inserted shape so the service can
    /// emit `BracketPlaced` without a re-read.
    pub async fn insert(&self, record: BracketGroupRecord) -> StorageResult<()> {
        let target_ids_json = serde_json::to_string(&record.target_order_ids)
            .map_err(|e| crate::storage::error::StorageError::Migration(e.to_string()))?;
        let targets_json = serde_json::to_string(&record.targets)
            .map_err(|e| crate::storage::error::StorageError::Migration(e.to_string()))?;

        self.db
            .with_conn(move |conn| {
                conn.execute(
                    "INSERT OR IGNORE INTO bracket_groups (
                        parent_order_id, setup_id, intent_id, account, symbol,
                        direction, parent_qty, system_qty, qty_override_reason,
                        entry_limit_cents, stop_order_id, stop_price_cents,
                        target_order_ids_json, targets_json,
                        placed_at, last_status, last_status_at
                     ) VALUES (
                        ?1, ?2, ?3, ?4, ?5,
                        ?6, ?7, ?8, ?9,
                        ?10, ?11, ?12,
                        ?13, ?14,
                        ?15, ?16, ?17
                     )",
                    params![
                        record.parent_order_id,
                        record.setup_id,
                        record.intent_id,
                        record.account,
                        record.symbol,
                        record.direction,
                        record.parent_qty as i64,
                        record.system_qty as i64,
                        record.qty_override_reason,
                        record.entry_limit_cents,
                        record.stop_order_id,
                        record.stop_price_cents,
                        target_ids_json,
                        targets_json,
                        record.placed_at.to_rfc3339(),
                        record.last_status.as_str(),
                        record.last_status_at.to_rfc3339(),
                    ],
                )?;
                Ok(())
            })
            .await
    }

    /// Read a single bracket by parent order id. Returns `None` if the
    /// id was never persisted.
    pub async fn get(&self, parent_order_id: i32) -> StorageResult<Option<BracketGroupRecord>> {
        self.db
            .with_conn(move |conn| {
                conn.query_row(SELECT_COLUMNS, params![parent_order_id], map_row)
                    .optional()
                    .map_err(Into::into)
            })
            .await
    }

    /// Read every bracket whose `setup_id` matches. Used by the
    /// trader-profile rollup; ordered most-recent-first.
    #[allow(dead_code)] // exercised by tests + reserved for the trader-profile surface
    pub async fn list_for_setup(&self, setup_id: i64) -> StorageResult<Vec<BracketGroupRecord>> {
        self.db
            .with_conn(move |conn| {
                let mut stmt = conn.prepare(SELECT_BY_SETUP)?;
                let rows = stmt
                    .query_map(params![setup_id], map_row)?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await
    }

    /// Phase 7 — list every bracket whose `last_status` is in the
    /// supplied set. Powers the `BracketReviser`'s sweep. Newest
    /// first. `statuses` may be empty → returns Vec::new().
    pub async fn list_by_statuses(
        &self,
        statuses: Vec<crate::services::order_ticket::types::BracketStatus>,
    ) -> StorageResult<Vec<BracketGroupRecord>> {
        if statuses.is_empty() {
            return Ok(Vec::new());
        }
        let status_strs: Vec<String> = statuses
            .into_iter()
            .map(|s| s.as_str().to_string())
            .collect();
        self.db
            .with_conn(move |conn| {
                let placeholders = (0..status_strs.len())
                    .map(|i| format!("?{}", i + 1))
                    .collect::<Vec<_>>()
                    .join(",");
                let sql = format!(
                    "SELECT \
                        parent_order_id, setup_id, intent_id, account, symbol, \
                        direction, parent_qty, system_qty, qty_override_reason, \
                        entry_limit_cents, stop_order_id, stop_price_cents, \
                        target_order_ids_json, targets_json, \
                        placed_at, last_status, last_status_at \
                     FROM bracket_groups WHERE last_status IN ({placeholders}) \
                     ORDER BY placed_at DESC"
                );
                let mut stmt = conn.prepare(&sql)?;
                let params_dyn: Vec<&dyn rusqlite::ToSql> = status_strs
                    .iter()
                    .map(|s| s as &dyn rusqlite::ToSql)
                    .collect();
                let rows = stmt
                    .query_map(params_dyn.as_slice(), map_row)?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await
    }

    /// Phase 7 — write the runtime trail state + the new stop price
    /// for `parent_order_id`. Atomic: stop price (in cents) and the
    /// JSON-encoded chandelier state move together so the reviser's
    /// next poll sees a consistent picture.
    pub async fn update_trail_state(
        &self,
        parent_order_id: i32,
        new_stop_price: f64,
        state: &crate::strategies::exits::ChandelierState,
    ) -> StorageResult<bool> {
        let new_stop_cents = (new_stop_price * 100.0).round() as i64;
        let state_json = serde_json::to_string(state)
            .map_err(|e| crate::storage::error::StorageError::Migration(e.to_string()))?;
        self.db
            .with_conn(move |conn| {
                let n = conn.execute(
                    "UPDATE bracket_groups \
                     SET stop_price_cents = ?1, trail_state_json = ?2 \
                     WHERE parent_order_id = ?3",
                    rusqlite::params![new_stop_cents, state_json, parent_order_id],
                )?;
                Ok(n > 0)
            })
            .await
    }

    /// Phase 7 — read back the persisted trail state for a bracket.
    /// `Ok(None)` ↔ no trail state yet (the reviser hasn't seeded it).
    pub async fn get_trail_state(
        &self,
        parent_order_id: i32,
    ) -> StorageResult<Option<crate::strategies::exits::ChandelierState>> {
        let raw: Option<Option<String>> = self
            .db
            .with_conn(move |conn| {
                conn.query_row(
                    "SELECT trail_state_json FROM bracket_groups WHERE parent_order_id = ?1",
                    rusqlite::params![parent_order_id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(Into::into)
            })
            .await?;
        let Some(Some(s)) = raw else { return Ok(None) };
        let st = serde_json::from_str(&s)
            .map_err(|e| crate::storage::error::StorageError::Migration(e.to_string()))?;
        Ok(Some(st))
    }

    /// Update `last_status` + `last_status_at`. Used by the cancel
    /// command and (later) the fill-status reconciler.
    pub async fn update_status(
        &self,
        parent_order_id: i32,
        status: BracketStatus,
        at: DateTime<Utc>,
    ) -> StorageResult<bool> {
        self.db
            .with_conn(move |conn| {
                let n = conn.execute(
                    "UPDATE bracket_groups
                     SET last_status = ?1, last_status_at = ?2
                     WHERE parent_order_id = ?3",
                    params![status.as_str(), at.to_rfc3339(), parent_order_id],
                )?;
                Ok(n > 0)
            })
            .await
    }
}

const SELECT_COLUMNS: &str = "SELECT
    parent_order_id, setup_id, intent_id, account, symbol,
    direction, parent_qty, system_qty, qty_override_reason,
    entry_limit_cents, stop_order_id, stop_price_cents,
    target_order_ids_json, targets_json,
    placed_at, last_status, last_status_at
 FROM bracket_groups WHERE parent_order_id = ?1";

#[allow(dead_code)] // pairs with `list_for_setup`
const SELECT_BY_SETUP: &str = "SELECT
    parent_order_id, setup_id, intent_id, account, symbol,
    direction, parent_qty, system_qty, qty_override_reason,
    entry_limit_cents, stop_order_id, stop_price_cents,
    target_order_ids_json, targets_json,
    placed_at, last_status, last_status_at
 FROM bracket_groups WHERE setup_id = ?1
 ORDER BY placed_at DESC";

fn map_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<BracketGroupRecord> {
    let target_ids_s: String = row.get(12)?;
    let targets_s: String = row.get(13)?;
    let placed_at_s: String = row.get(14)?;
    let last_status_s: String = row.get(15)?;
    let last_status_at_s: String = row.get(16)?;
    let parent_qty_i: i64 = row.get(6)?;
    let system_qty_i: i64 = row.get(7)?;

    let target_order_ids: Vec<i32> = serde_json::from_str(&target_ids_s).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(12, rusqlite::types::Type::Text, Box::new(e))
    })?;
    let targets: Vec<TargetSpec> = serde_json::from_str(&targets_s).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(13, rusqlite::types::Type::Text, Box::new(e))
    })?;
    let placed_at = parse_rfc3339(&placed_at_s, 14)?;
    let last_status = BracketStatus::parse(&last_status_s).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            15,
            rusqlite::types::Type::Text,
            format!("unknown bracket_groups.last_status '{last_status_s}'").into(),
        )
    })?;
    let last_status_at = parse_rfc3339(&last_status_at_s, 16)?;

    Ok(BracketGroupRecord {
        parent_order_id: row.get(0)?,
        setup_id: row.get(1)?,
        intent_id: row.get(2)?,
        account: row.get(3)?,
        symbol: row.get(4)?,
        direction: row.get(5)?,
        parent_qty: parent_qty_i.max(0) as u32,
        system_qty: system_qty_i.max(0) as u32,
        qty_override_reason: row.get(8)?,
        entry_limit_cents: row.get(9)?,
        stop_order_id: row.get(10)?,
        stop_price_cents: row.get(11)?,
        target_order_ids,
        targets,
        placed_at,
        last_status,
        last_status_at,
    })
}

fn parse_rfc3339(s: &str, col: usize) -> rusqlite::Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(col, rusqlite::types::Type::Text, Box::new(e))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn fresh_store() -> (NamedTempFile, BracketGroupStore) {
        let tmp = NamedTempFile::new().unwrap();
        let db = Arc::new(Db::open(tmp.path()).unwrap());
        (tmp, BracketGroupStore::new(db))
    }

    fn sample_record() -> BracketGroupRecord {
        let now = Utc::now();
        BracketGroupRecord {
            parent_order_id: 1001,
            setup_id: 42,
            intent_id: "intent_s42_xyz".to_string(),
            account: "DU1".to_string(),
            symbol: "AAPL".to_string(),
            direction: "long".to_string(),
            parent_qty: 100,
            system_qty: 100,
            qty_override_reason: None,
            entry_limit_cents: 15_000,
            stop_order_id: 1002,
            stop_price_cents: 14_800,
            target_order_ids: vec![1003, 1004, 1005],
            targets: vec![
                TargetSpec {
                    label: "1R".to_string(),
                    price: 150.20,
                    qty: 50,
                    qty_pct: 50,
                },
                TargetSpec {
                    label: "2R".to_string(),
                    price: 150.40,
                    qty: 30,
                    qty_pct: 30,
                },
                TargetSpec {
                    label: "3R".to_string(),
                    price: 150.60,
                    qty: 20,
                    qty_pct: 20,
                },
            ],
            placed_at: now,
            last_status: BracketStatus::Open,
            last_status_at: now,
        }
    }

    /// Seed minimum schema so FK-checked inserts don't panic on the
    /// `setup_id` / `intent_id` references. `setups.symbol` FKs into
    /// `tracked_tickers`, so the watchlist row has to land first.
    /// Uses raw SQL to avoid dragging in the full TrackerService +
    /// TcaService graph.
    async fn seed_fk_targets(db: &Db) {
        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO tracked_tickers (
                    symbol, source, status, tags, added_at
                 ) VALUES ('AAPL', 'manual', 'watching', '[]', 1234567000)",
                [],
            )?;
            conn.execute(
                "INSERT INTO setups (
                    id, symbol, strategy, direction, detected_at,
                    trigger_price, stop_price, targets, raw_signals,
                    status
                 ) VALUES (
                    42, 'AAPL', 'breakout', 'long', 1234567890,
                    150.0, 148.0, '[]', '{}',
                    'active'
                 )",
                [],
            )?;
            conn.execute(
                "INSERT INTO order_intents (
                    intent_id, setup_id, account, symbol, side, qty,
                    intended_price_cents, intended_price_source,
                    posted_at, expires_at
                 ) VALUES (
                    'intent_s42_xyz', 42, 'DU1', 'AAPL', 'buy', 100,
                    15000, 'trigger_price',
                    '2026-05-05T13:00:00Z', '2026-05-05T14:00:00Z'
                 )",
                [],
            )?;
            Ok(())
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn insert_and_get_round_trips() {
        let (_tmp, store) = fresh_store();
        seed_fk_targets(&store.db).await;
        store.insert(sample_record()).await.unwrap();
        let got = store.get(1001).await.unwrap().unwrap();
        assert_eq!(got.parent_order_id, 1001);
        assert_eq!(got.target_order_ids, vec![1003, 1004, 1005]);
        assert_eq!(got.targets.len(), 3);
        assert_eq!(got.targets[0].qty_pct, 50);
        assert_eq!(got.last_status, BracketStatus::Open);
    }

    #[tokio::test]
    async fn insert_is_idempotent() {
        let (_tmp, store) = fresh_store();
        seed_fk_targets(&store.db).await;
        store.insert(sample_record()).await.unwrap();
        let mut second = sample_record();
        second.symbol = "MSFT".to_string();
        store.insert(second).await.unwrap();
        let got = store.get(1001).await.unwrap().unwrap();
        assert_eq!(got.symbol, "AAPL"); // first write wins
    }

    #[tokio::test]
    async fn update_status_flips_status_and_timestamp() {
        let (_tmp, store) = fresh_store();
        seed_fk_targets(&store.db).await;
        store.insert(sample_record()).await.unwrap();
        let later = Utc::now();
        let updated = store
            .update_status(1001, BracketStatus::Canceled, later)
            .await
            .unwrap();
        assert!(updated);
        let got = store.get(1001).await.unwrap().unwrap();
        assert_eq!(got.last_status, BracketStatus::Canceled);
    }

    #[tokio::test]
    async fn list_for_setup_returns_chronological() {
        let (_tmp, store) = fresh_store();
        seed_fk_targets(&store.db).await;
        let mut a = sample_record();
        a.parent_order_id = 1001;
        a.placed_at = Utc::now() - chrono::Duration::minutes(10);
        let mut b = sample_record();
        b.parent_order_id = 2001;
        b.placed_at = Utc::now();
        store.insert(a).await.unwrap();
        store.insert(b).await.unwrap();
        let rows = store.list_for_setup(42).await.unwrap();
        assert_eq!(rows.len(), 2);
        // DESC by placed_at — most-recent first.
        assert_eq!(rows[0].parent_order_id, 2001);
    }
}
