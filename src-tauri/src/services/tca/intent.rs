//! Phase 2 — `OrderIntentStore`: persistence for `order_intents`.
//!
//! All writes go through here. The matcher (`matcher.rs`) is pure
//! and knows nothing about SQLite; this store is the only thing that
//! mutates `order_intents` rows or stamps the linkage columns on
//! `executions`.

use std::sync::Arc;

use chrono::{DateTime, Duration, NaiveDate, Utc};
use rusqlite::{params, OptionalExtension};

use crate::storage::error::Result as StorageResult;
use crate::storage::Db;

use super::types::{
    IntendedPriceSource, IntentSide, IntentStatus, LinkageDecision, OrderIntent,
};

/// Builder for a new intent. Fields validated at insert time.
#[derive(Debug, Clone)]
pub struct NewOrderIntent {
    pub intent_id: String,
    pub setup_id: Option<i64>,
    pub account: String,
    pub symbol: String,
    pub side: IntentSide,
    pub qty: f64,
    pub intended_price_cents: i64,
    pub intended_price_source: IntendedPriceSource,
    pub posted_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct OrderIntentStore {
    db: Arc<Db>,
}

impl OrderIntentStore {
    pub fn new(db: Arc<Db>) -> Self {
        Self { db }
    }

    /// Insert a new intent. Idempotent on `intent_id` — re-inserting
    /// the same id is a no-op (lets the trader's confirm-then-retry
    /// flow play through cleanly).
    pub async fn insert(&self, intent: NewOrderIntent) -> StorageResult<()> {
        self.db
            .with_conn(move |conn| {
                conn.execute(
                    "INSERT OR IGNORE INTO order_intents (
                        intent_id, setup_id, account, symbol, side, qty,
                        intended_price_cents, intended_price_source,
                        posted_at, expires_at, status, matched_qty
                     ) VALUES (
                        ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'open', 0.0
                     )",
                    params![
                        intent.intent_id,
                        intent.setup_id,
                        intent.account,
                        intent.symbol,
                        intent.side.as_str(),
                        intent.qty,
                        intent.intended_price_cents,
                        intent.intended_price_source.as_str(),
                        intent.posted_at.to_rfc3339(),
                        intent.expires_at.to_rfc3339(),
                    ],
                )?;
                Ok(())
            })
            .await
    }

    /// Fetch a single intent by id.
    pub async fn get(&self, intent_id: &str) -> StorageResult<Option<OrderIntent>> {
        let id = intent_id.to_string();
        self.db
            .with_conn(move |conn| {
                conn.query_row(
                    "SELECT intent_id, setup_id, account, symbol, side, qty,
                            intended_price_cents, intended_price_source,
                            posted_at, expires_at, status, matched_qty
                     FROM order_intents WHERE intent_id = ?1",
                    params![id],
                    map_row,
                )
                .optional()
                .map_err(Into::into)
            })
            .await
    }

    /// Find open intents matching the (account, symbol, side) shape
    /// of a fill. The matcher then re-checks the time window. The
    /// `idx_order_intents_open_lookup` partial index keeps this read
    /// O(matching open count) regardless of intent history depth.
    pub async fn find_open_for_fill(
        &self,
        account: &str,
        symbol: &str,
        side: IntentSide,
    ) -> StorageResult<Vec<OrderIntent>> {
        let account = account.to_string();
        let symbol = symbol.to_string();
        let side_str = side.as_str();
        self.db
            .with_conn(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT intent_id, setup_id, account, symbol, side, qty,
                            intended_price_cents, intended_price_source,
                            posted_at, expires_at, status, matched_qty
                     FROM order_intents
                     WHERE status = 'open'
                       AND account = ?1 AND symbol = ?2 AND side = ?3
                     ORDER BY posted_at ASC",
                )?;
                let rows = stmt
                    .query_map(params![account, symbol, side_str], map_row)?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await
    }

    /// Apply a matcher decision: update the intent's matched qty and
    /// status, stamp the linkage columns on the matching `executions`
    /// row. Single transaction — the two writes are atomic.
    ///
    /// Returns `true` when the executions row was updated. A `false`
    /// means the exec_id wasn't found (race condition; surfaced as a
    /// log-and-skip, not an error).
    pub async fn apply_linkage(&self, decision: LinkageDecision) -> StorageResult<bool> {
        self.db
            .with_conn(move |conn| {
                let tx = conn.transaction()?;
                // Stamp the executions row first. The `intent_id IS
                // NULL` filter makes this a no-op for already-linked
                // fills, which is the only way `attach_fills_for_*`
                // gets re-called on the same fill (idempotent
                // sweep). When the UPDATE matches zero rows, also
                // skip the intent bump — otherwise a re-sweep
                // double-counts `matched_qty`.
                let n = tx.execute(
                    "UPDATE executions
                     SET intent_id = ?1,
                         setup_id = ?2,
                         intended_price_cents = ?3,
                         intended_price_source = ?4,
                         slippage_bps = ?5,
                         slippage_signed = ?6
                     WHERE exec_id = ?7
                       AND intent_id IS NULL",
                    params![
                        decision.intent_id,
                        decision.setup_id,
                        decision.intended_price_cents,
                        decision.intended_price_source.as_str(),
                        decision.slippage_bps,
                        decision.slippage_signed,
                        decision.exec_id,
                    ],
                )?;
                if n == 0 {
                    tx.commit()?;
                    return Ok(false);
                }
                let intent_qty: f64 = tx.query_row(
                    "SELECT qty FROM order_intents WHERE intent_id = ?1",
                    params![decision.intent_id],
                    |r| r.get(0),
                )?;
                let new_status = if decision.new_matched_qty + 1e-9 >= intent_qty {
                    IntentStatus::Matched
                } else {
                    IntentStatus::Open
                };
                tx.execute(
                    "UPDATE order_intents
                     SET matched_qty = ?1, status = ?2
                     WHERE intent_id = ?3",
                    params![
                        decision.new_matched_qty,
                        new_status.as_str(),
                        decision.intent_id
                    ],
                )?;
                tx.commit()?;
                Ok(true)
            })
            .await
    }

    /// Sweep: mark any open intent whose `expires_at` is in the past
    /// (relative to `now`) as `expired`. Idempotent — re-running
    /// only flips rows still in `open`. Returns the number flipped.
    pub async fn expire_stale(&self, now: DateTime<Utc>) -> StorageResult<usize> {
        let now_iso = now.to_rfc3339();
        self.db
            .with_conn(move |conn| {
                let n = conn.execute(
                    "UPDATE order_intents
                     SET status = 'expired'
                     WHERE status = 'open' AND expires_at < ?1",
                    params![now_iso],
                )?;
                Ok(n)
            })
            .await
    }

    /// Diagnostic: count open intents for an account. Used by the
    /// future "you have N unmatched intents" surface.
    #[allow(dead_code)]
    pub async fn open_count(&self, account: &str) -> StorageResult<usize> {
        let account = account.to_string();
        self.db
            .with_conn(move |conn| {
                let n: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM order_intents
                     WHERE status = 'open' AND account = ?1",
                    params![account],
                    |r| r.get(0),
                )?;
                Ok(n as usize)
            })
            .await
    }
}

fn map_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<OrderIntent> {
    let posted_at_str: String = row.get(8)?;
    let expires_at_str: String = row.get(9)?;
    let posted_at = parse_rfc3339(&posted_at_str, 8)?;
    let expires_at = parse_rfc3339(&expires_at_str, 9)?;
    let side_str: String = row.get(4)?;
    let source_str: String = row.get(7)?;
    let status_str: String = row.get(10)?;
    Ok(OrderIntent {
        intent_id: row.get(0)?,
        setup_id: row.get(1)?,
        account: row.get(2)?,
        symbol: row.get(3)?,
        side: IntentSide::parse(&side_str).unwrap_or(IntentSide::Buy),
        qty: row.get(5)?,
        intended_price_cents: row.get(6)?,
        intended_price_source: IntendedPriceSource::parse(&source_str)
            .unwrap_or(IntendedPriceSource::Manual),
        posted_at,
        expires_at,
        status: IntentStatus::parse(&status_str).unwrap_or(IntentStatus::Open),
        matched_qty: row.get(11)?,
    })
}

fn parse_rfc3339(s: &str, col_idx: usize) -> rusqlite::Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(
                col_idx,
                rusqlite::types::Type::Text,
                Box::new(e),
            )
        })
}

/// Default expiry helper. `is_market` toggles the tighter 5-min
/// market window vs the 60-min limit window. Mirrors
/// `MatchWindow::default()` so callers that don't override end up
/// in the same place.
pub fn default_expiry(now: DateTime<Utc>, is_market: bool) -> DateTime<Utc> {
    let minutes = if is_market { 5 } else { 60 };
    now + Duration::minutes(minutes)
}

/// Default for the trading-day boundary used by attribution. Kept
/// here so the store and the attribution module agree on the
/// half-open `[start, end)` ET-day convention.
#[allow(dead_code)]
pub fn et_day_iso(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    async fn fresh_store() -> (NamedTempFile, OrderIntentStore) {
        let tmp = NamedTempFile::new().unwrap();
        let db = Arc::new(Db::open(tmp.path()).unwrap());
        (tmp, OrderIntentStore::new(db))
    }

    fn make_new_intent(id: &str, qty: f64) -> NewOrderIntent {
        let now = Utc::now();
        NewOrderIntent {
            intent_id: id.to_string(),
            // Unit tests don't seed `setups`; the FK still allows
            // NULL, so the store layer is exercised without dragging
            // in the full setups schema.
            setup_id: None,
            account: "DU1".to_string(),
            symbol: "AAPL".to_string(),
            side: IntentSide::Buy,
            qty,
            intended_price_cents: 100_00,
            intended_price_source: IntendedPriceSource::TriggerPrice,
            posted_at: now,
            expires_at: now + Duration::minutes(60),
        }
    }

    #[tokio::test]
    async fn insert_and_get_round_trips() {
        let (_tmp, store) = fresh_store().await;
        store.insert(make_new_intent("i_1", 100.0)).await.unwrap();
        let got = store.get("i_1").await.unwrap().expect("stored");
        assert_eq!(got.qty, 100.0);
        assert_eq!(got.status, IntentStatus::Open);
        assert_eq!(got.matched_qty, 0.0);
    }

    #[tokio::test]
    async fn insert_is_idempotent() {
        let (_tmp, store) = fresh_store().await;
        store.insert(make_new_intent("i_1", 100.0)).await.unwrap();
        // Second insert with same id is a no-op (no error, no overwrite).
        let mut second = make_new_intent("i_1", 999.0);
        second.symbol = "MSFT".to_string();
        store.insert(second).await.unwrap();
        let got = store.get("i_1").await.unwrap().unwrap();
        assert_eq!(got.qty, 100.0);
        assert_eq!(got.symbol, "AAPL");
    }

    #[tokio::test]
    async fn find_open_filters_by_shape() {
        let (_tmp, store) = fresh_store().await;
        store.insert(make_new_intent("i_buy", 100.0)).await.unwrap();
        let mut sell = make_new_intent("i_sell", 100.0);
        sell.side = IntentSide::Sell;
        store.insert(sell).await.unwrap();
        let buys = store
            .find_open_for_fill("DU1", "AAPL", IntentSide::Buy)
            .await
            .unwrap();
        assert_eq!(buys.len(), 1);
        assert_eq!(buys[0].intent_id, "i_buy");
    }

    #[tokio::test]
    async fn expire_stale_flips_only_past_open() {
        let (_tmp, store) = fresh_store().await;
        let mut ancient = make_new_intent("i_ancient", 100.0);
        ancient.expires_at = Utc::now() - Duration::minutes(1);
        store.insert(ancient).await.unwrap();
        store.insert(make_new_intent("i_fresh", 100.0)).await.unwrap();
        let n = store.expire_stale(Utc::now()).await.unwrap();
        assert_eq!(n, 1);
        let ancient = store.get("i_ancient").await.unwrap().unwrap();
        let fresh = store.get("i_fresh").await.unwrap().unwrap();
        assert_eq!(ancient.status, IntentStatus::Expired);
        assert_eq!(fresh.status, IntentStatus::Open);
    }
}
