//! Phase 6 — read-only bars-cache reader for the backtester.
//!
//! Backtester replay must NEVER call `IbkrClient`. The reader trait is
//! the seam: production reads the same `bars_cache` table the live
//! `HistoricalDataService` populates, but does NOT trigger an IBKR
//! fetch on a miss — bars must already be cached by a prior live
//! session, the trader's morning-prime sweep, or the manual
//! `tracker_fetch_bars` Tauri command.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use rusqlite::{params, OptionalExtension};

use crate::ibkr::types::historical::{parse_ibkr_time, BarSize, HistoricalBar};
use crate::storage::error::Result as StorageResult;
use crate::storage::Db;

#[async_trait]
pub trait BarsReader: Send + Sync {
    /// Return all cached bars for `(symbol, bar_size)` whose
    /// `bar_time` falls in `[start_unix, end_unix_inclusive]`. Sort
    /// ascending. Empty when the cache has no rows.
    async fn read_window(
        &self,
        symbol: &str,
        bar_size: BarSize,
        start_unix: i64,
        end_unix_inclusive: i64,
    ) -> StorageResult<Vec<HistoricalBar>>;
}

/// Production `BarsReader` — reads `bars_cache` directly. Mirrors the
/// `HistoricalDataService::cache::read_cache` query so the bytes a
/// live session caches are byte-identical to what the backtester
/// reads.
pub struct DbBarsReader {
    db: Arc<Db>,
}

impl DbBarsReader {
    pub fn new(db: Arc<Db>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl BarsReader for DbBarsReader {
    async fn read_window(
        &self,
        symbol: &str,
        bar_size: BarSize,
        start_unix: i64,
        end_unix_inclusive: i64,
    ) -> StorageResult<Vec<HistoricalBar>> {
        let symbol = symbol.to_string();
        let bar_size_str = bar_size.as_str().to_string();
        self.db
            .with_conn(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT bar_time, open, high, low, close, volume, wap \
                     FROM bars_cache \
                     WHERE symbol = ?1 AND bar_size = ?2 \
                       AND bar_time >= ?3 AND bar_time <= ?4 \
                     ORDER BY bar_time ASC",
                )?;
                let rows = stmt.query_map(
                    params![symbol, bar_size_str, start_unix, end_unix_inclusive],
                    |row| {
                        let ts: i64 = row.get(0)?;
                        let open: f64 = row.get(1)?;
                        let high: f64 = row.get(2)?;
                        let low: f64 = row.get(3)?;
                        let close: f64 = row.get(4)?;
                        let volume: i64 = row.get(5)?;
                        let wap: Option<f64> = row.get(6)?;
                        Ok(HistoricalBar {
                            time: format_unix_to_ibkr(ts),
                            open,
                            high,
                            low,
                            close,
                            volume,
                            wap: wap.unwrap_or(0.0),
                            count: 0,
                        })
                    },
                )?;
                let mut out = Vec::new();
                for row in rows {
                    out.push(row?);
                }
                Ok(out)
            })
            .await
    }
}

/// Render a unix-second timestamp back into the IBKR string format —
/// shared with `historical_data_service::cache`.
fn format_unix_to_ibkr(ts: i64) -> String {
    let dt = match Utc.timestamp_opt(ts, 0).single() {
        Some(d) => d,
        None => return String::new(),
    };
    if dt.format("%H:%M:%S").to_string() == "00:00:00" {
        dt.format("%Y%m%d").to_string()
    } else {
        dt.format("%Y%m%d %H:%M:%S").to_string()
    }
}

/// Sanity helper: given a NaiveDate range, return the corresponding
/// (start, end_inclusive) UNIX-second bounds. Daily bars in
/// `bars_cache` are stored at midnight UTC of their date.
pub fn date_range_to_unix(from: NaiveDate, to_inclusive: NaiveDate) -> (i64, i64) {
    let start = Utc
        .from_utc_datetime(&from.and_hms_opt(0, 0, 0).expect("midnight valid"))
        .timestamp();
    let end = Utc
        .from_utc_datetime(&to_inclusive.and_hms_opt(23, 59, 59).expect("eod valid"))
        .timestamp();
    (start, end)
}

/// Convert a `HistoricalBar`'s IBKR time string back to a UTC moment.
/// Daily bars round to midnight; intraday bars carry HH:MM:SS.
pub fn bar_time_utc(bar: &HistoricalBar) -> Option<DateTime<Utc>> {
    let ts = parse_ibkr_time(&bar.time).ok()?;
    Utc.timestamp_opt(ts, 0).single()
}

/// Insert one bar row directly into `bars_cache`. Test-only helper —
/// production writes go through `HistoricalDataService::cache`.
#[cfg(test)]
pub async fn insert_bar(
    db: &Db,
    symbol: &str,
    bar_size: BarSize,
    bar: &HistoricalBar,
) -> StorageResult<()> {
    let symbol = symbol.to_string();
    let bar_size_str = bar_size.as_str().to_string();
    let bar = bar.clone();
    db.with_conn(move |conn| {
        let ts = parse_ibkr_time(&bar.time).map_err(|e| {
            crate::storage::StorageError::Migration(format!("bad bar time '{}': {e}", bar.time))
        })?;
        conn.execute(
            "INSERT OR REPLACE INTO bars_cache \
             (symbol, bar_size, bar_time, open, high, low, close, volume, wap) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                symbol,
                bar_size_str,
                ts,
                bar.open,
                bar.high,
                bar.low,
                bar.close,
                bar.volume,
                bar.wap
            ],
        )?;
        Ok(())
    })
    .await
}

#[allow(dead_code)] // surfaced when MCP / Tauri commands query existence
pub async fn count_bars(db: &Db, symbol: &str, bar_size: BarSize) -> StorageResult<i64> {
    let symbol = symbol.to_string();
    let bar_size_str = bar_size.as_str().to_string();
    db.with_conn(move |conn| {
        let n: Option<i64> = conn
            .query_row(
                "SELECT COUNT(*) FROM bars_cache WHERE symbol = ?1 AND bar_size = ?2",
                params![symbol, bar_size_str],
                |row| row.get(0),
            )
            .optional()?;
        Ok(n.unwrap_or(0))
    })
    .await
}
