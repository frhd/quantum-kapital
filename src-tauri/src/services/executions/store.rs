//! `ExecutionsStore` — writer + reader for the `executions` table.
//!
//! Idempotent UPSERT keyed on `exec_id`. Late-arriving commission
//! reports patch existing rows without overwriting populated values
//! (first non-NULL commission wins).

use std::sync::Arc;

use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use chrono_tz::America::New_York;
use rusqlite::{params, OptionalExtension};

use crate::ibkr::types::{ExecutionSide, IbkrExecution};
use crate::storage::error::Result as StorageResult;
use crate::storage::Db;

#[derive(Debug, Default, Clone)]
pub struct RecordSummary {
    pub inserted: usize,
    pub commission_patched: usize,
    pub skipped_redundant: usize,
}

#[derive(Debug, Default, Clone)]
pub struct BackfillSummary {
    pub inserted: usize,
    pub skipped_existing: usize,
    pub skipped_live_match: usize,
}

#[derive(Clone)]
pub struct ExecutionsStore {
    db: Arc<Db>,
}

impl ExecutionsStore {
    pub fn new(db: Arc<Db>) -> Self {
        Self { db }
    }

    /// UPSERT a batch of fills.
    ///
    /// - New `exec_id` ⇒ INSERT.
    /// - Existing `exec_id` with `commission IS NULL` and incoming
    ///   `commission IS Some(_)` ⇒ UPDATE commission/realized_pnl/
    ///   commission_currency, stamp `commission_patched_at`.
    /// - Existing `exec_id` with `commission IS Some(_)` ⇒ no-op
    ///   (first non-NULL wins).
    pub async fn record(&self, rows: &[IbkrExecution]) -> StorageResult<RecordSummary> {
        if rows.is_empty() {
            return Ok(RecordSummary::default());
        }
        let owned: Vec<IbkrExecution> = rows.to_vec();
        self.db
            .with_conn(move |conn| {
                let tx = conn.transaction()?;
                let mut summary = RecordSummary::default();
                for row in &owned {
                    let existing: Option<Option<f64>> = tx
                        .query_row(
                            "SELECT commission FROM executions WHERE exec_id = ?1",
                            params![row.exec_id],
                            |r| r.get::<_, Option<f64>>(0),
                        )
                        .optional()?;
                    match existing {
                        None => {
                            tx.execute(
                                "INSERT INTO executions (
                                    exec_id, account, symbol, contract_type, expiry,
                                    strike, \"right\", multiplier, side, qty, avg_price,
                                    currency, exec_time, order_id, commission,
                                    realized_pnl, commission_currency
                                 ) VALUES (
                                    ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11,
                                    ?12, ?13, ?14, ?15, ?16, ?17
                                 )",
                                params![
                                    row.exec_id,
                                    row.account,
                                    row.symbol,
                                    row.contract_type,
                                    row.expiry.map(|d| d.format("%Y-%m-%d").to_string()),
                                    row.strike,
                                    row.right,
                                    row.multiplier,
                                    side_to_str(&row.side),
                                    row.qty,
                                    row.avg_price,
                                    row.currency,
                                    row.exec_time.to_rfc3339(),
                                    row.order_id,
                                    row.commission,
                                    row.realized_pnl,
                                    row.commission_currency,
                                ],
                            )?;
                            summary.inserted += 1;
                        }
                        Some(None) if row.commission.is_some() => {
                            tx.execute(
                                "UPDATE executions
                                 SET commission = ?1,
                                     realized_pnl = COALESCE(?2, realized_pnl),
                                     commission_currency = COALESCE(?3, commission_currency),
                                     commission_patched_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
                                 WHERE exec_id = ?4",
                                params![
                                    row.commission,
                                    row.realized_pnl,
                                    row.commission_currency,
                                    row.exec_id
                                ],
                            )?;
                            summary.commission_patched += 1;
                        }
                        _ => {
                            summary.skipped_redundant += 1;
                        }
                    }
                }
                tx.commit()?;
                Ok(summary)
            })
            .await
    }

    /// UPSERT a batch of Flex-Web-Service backfill rows.
    ///
    /// Each row's `exec_id` MUST be prefixed with `flex:` (e.g. `flex:8849853516`)
    /// so it can never collide with a live `reqExecutions` exec_id.
    ///
    /// Three outcomes per row:
    /// - `exec_id` already in the store ⇒ no-op (idempotent re-run).
    /// - A `source='live'` row matches the same fill by composite key
    ///   `(account, symbol, side, qty, avg_price)` within ±60s of `exec_time`
    ///   ⇒ skip — the live row is authoritative.
    /// - Otherwise ⇒ INSERT with `source='flex'`.
    pub async fn record_backfill(&self, rows: &[IbkrExecution]) -> StorageResult<BackfillSummary> {
        if rows.is_empty() {
            return Ok(BackfillSummary::default());
        }
        let owned: Vec<IbkrExecution> = rows.to_vec();
        self.db
            .with_conn(move |conn| {
                let tx = conn.transaction()?;
                let mut summary = BackfillSummary::default();
                for row in &owned {
                    let exists: i64 = tx.query_row(
                        "SELECT COUNT(*) FROM executions WHERE exec_id = ?1",
                        params![row.exec_id],
                        |r| r.get(0),
                    )?;
                    if exists > 0 {
                        summary.skipped_existing += 1;
                        continue;
                    }
                    let exec_time_iso = row.exec_time.to_rfc3339();
                    let live_match: i64 = tx.query_row(
                        "SELECT COUNT(*) FROM executions
                         WHERE source = 'live'
                           AND account = ?1
                           AND symbol = ?2
                           AND side = ?3
                           AND ABS(qty - ?4) < 1e-6
                           AND ABS(avg_price - ?5) < 1e-6
                           AND ABS((julianday(exec_time) - julianday(?6)) * 86400) < 60",
                        params![
                            row.account,
                            row.symbol,
                            side_to_str(&row.side),
                            row.qty,
                            row.avg_price,
                            exec_time_iso,
                        ],
                        |r| r.get(0),
                    )?;
                    if live_match > 0 {
                        summary.skipped_live_match += 1;
                        continue;
                    }
                    tx.execute(
                        "INSERT INTO executions (
                            exec_id, account, symbol, contract_type, expiry,
                            strike, \"right\", multiplier, side, qty, avg_price,
                            currency, exec_time, order_id, commission,
                            realized_pnl, commission_currency, source
                         ) VALUES (
                            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11,
                            ?12, ?13, ?14, ?15, ?16, ?17, 'flex'
                         )",
                        params![
                            row.exec_id,
                            row.account,
                            row.symbol,
                            row.contract_type,
                            row.expiry.map(|d| d.format("%Y-%m-%d").to_string()),
                            row.strike,
                            row.right,
                            row.multiplier,
                            side_to_str(&row.side),
                            row.qty,
                            row.avg_price,
                            row.currency,
                            exec_time_iso,
                            row.order_id,
                            row.commission,
                            row.realized_pnl,
                            row.commission_currency,
                        ],
                    )?;
                    summary.inserted += 1;
                }
                tx.commit()?;
                Ok(summary)
            })
            .await
    }

    /// Read fills for an ET trading day, optionally filtered by symbol.
    pub async fn query(
        &self,
        account: &str,
        date: NaiveDate,
        symbol: Option<&str>,
    ) -> StorageResult<Vec<IbkrExecution>> {
        let (start_utc, end_utc) = et_day_bounds_utc(date);
        let account = account.to_string();
        let symbol = symbol.map(|s| s.to_string());
        self.db
            .with_conn(move |conn| {
                let start = start_utc.to_rfc3339();
                let end = end_utc.to_rfc3339();
                let rows = match &symbol {
                    Some(sym) => {
                        let mut stmt = conn.prepare(
                            "SELECT exec_id, account, symbol, contract_type, expiry, strike,
                                    \"right\", multiplier, side, qty, avg_price, currency,
                                    exec_time, order_id, commission, realized_pnl,
                                    commission_currency
                             FROM executions
                             WHERE account = ?1 AND symbol = ?2
                               AND exec_time >= ?3 AND exec_time < ?4
                             ORDER BY exec_time ASC",
                        )?;
                        let mapped = stmt
                            .query_map(params![account, sym, start, end], map_row)?
                            .collect::<rusqlite::Result<Vec<_>>>()?;
                        mapped
                    }
                    None => {
                        let mut stmt = conn.prepare(
                            "SELECT exec_id, account, symbol, contract_type, expiry, strike,
                                    \"right\", multiplier, side, qty, avg_price, currency,
                                    exec_time, order_id, commission, realized_pnl,
                                    commission_currency
                             FROM executions
                             WHERE account = ?1
                               AND exec_time >= ?2 AND exec_time < ?3
                             ORDER BY exec_time ASC",
                        )?;
                        let mapped = stmt
                            .query_map(params![account, start, end], map_row)?
                            .collect::<rusqlite::Result<Vec<_>>>()?;
                        mapped
                    }
                };
                Ok(rows)
            })
            .await
    }

    /// Count fills with `commission IS NULL` since the given ET day.
    /// Observability hook — surfaces stuck rows during dogfooding.
    #[allow(dead_code)] // wired by Phase 2+
    pub async fn pending_commission_count(
        &self,
        account: &str,
        since: NaiveDate,
    ) -> StorageResult<usize> {
        let (start_utc, _) = et_day_bounds_utc(since);
        let account = account.to_string();
        self.db
            .with_conn(move |conn| {
                let n: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM executions
                     WHERE account = ?1 AND commission IS NULL AND exec_time >= ?2",
                    params![account, start_utc.to_rfc3339()],
                    |r| r.get(0),
                )?;
                Ok(n as usize)
            })
            .await
    }
}

fn side_to_str(side: &ExecutionSide) -> &'static str {
    match side {
        ExecutionSide::Bought => "bought",
        ExecutionSide::Sold => "sold",
    }
}

fn parse_side(s: &str) -> ExecutionSide {
    match s {
        "sold" => ExecutionSide::Sold,
        _ => ExecutionSide::Bought,
    }
}

fn map_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<IbkrExecution> {
    let exec_time_str: String = row.get(12)?;
    let exec_time = DateTime::parse_from_rfc3339(&exec_time_str)
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(12, rusqlite::types::Type::Text, Box::new(e))
        })?
        .with_timezone(&Utc);
    Ok(IbkrExecution {
        exec_id: row.get(0)?,
        account: row.get(1)?,
        symbol: row.get(2)?,
        contract_type: row.get(3)?,
        expiry: row
            .get::<_, Option<String>>(4)?
            .and_then(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok()),
        strike: row.get(5)?,
        right: row.get(6)?,
        multiplier: row.get(7)?,
        side: parse_side(&row.get::<_, String>(8)?),
        qty: row.get(9)?,
        avg_price: row.get(10)?,
        currency: row.get(11)?,
        exec_time,
        order_id: row.get(13)?,
        commission: row.get(14)?,
        realized_pnl: row.get(15)?,
        commission_currency: row.get(16)?,
    })
}

/// Convert an ET trading day to a `[start_utc, end_utc)` half-open
/// range. DST-correct via `chrono_tz::America::New_York`.
fn et_day_bounds_utc(date: NaiveDate) -> (DateTime<Utc>, DateTime<Utc>) {
    let day_start_naive = date.and_hms_opt(0, 0, 0).expect("midnight valid");
    let next_day_naive = (date + chrono::Duration::days(1))
        .and_hms_opt(0, 0, 0)
        .expect("next midnight valid");
    let start_et = New_York
        .from_local_datetime(&day_start_naive)
        .single()
        .expect("ET midnight unambiguous");
    let end_et = New_York
        .from_local_datetime(&next_day_naive)
        .single()
        .expect("ET next-midnight unambiguous");
    (start_et.with_timezone(&Utc), end_et.with_timezone(&Utc))
}
