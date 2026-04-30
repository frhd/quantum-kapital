//! SQLite-backed bars cache reads/writes for `HistoricalDataService`.
//!
//! The service module owns the orchestration (rate limiting, gap-fill,
//! merge); this module owns the SQL.

use chrono::{TimeZone, Utc};
use rusqlite;

use crate::ibkr::error::{IbkrError, Result as IbkrResult};
use crate::ibkr::types::historical::{parse_ibkr_time, BarSize, HistoricalBar};
use crate::storage::error::StorageError;
use crate::storage::Db;

pub(super) async fn read_cache(
    db: &Db,
    symbol: &str,
    bar_size: BarSize,
    start_unix: i64,
    end_unix: i64,
) -> IbkrResult<Vec<HistoricalBar>> {
    let symbol = symbol.to_string();
    let bar_size_str = bar_size.as_str().to_string();
    db.with_conn(move |conn| {
        let mut stmt = conn.prepare(
            "SELECT bar_time, open, high, low, close, volume, wap \
             FROM bars_cache \
             WHERE symbol = ?1 AND bar_size = ?2 \
               AND bar_time >= ?3 AND bar_time <= ?4 \
             ORDER BY bar_time ASC",
        )?;
        let rows = stmt.query_map(
            rusqlite::params![symbol, bar_size_str, start_unix, end_unix],
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
    .map_err(map_storage_err)
}

pub(super) async fn write_cache(
    db: &Db,
    symbol: &str,
    bar_size: BarSize,
    bars: &[HistoricalBar],
) -> IbkrResult<()> {
    if bars.is_empty() {
        return Ok(());
    }
    let symbol = symbol.to_string();
    let bar_size_str = bar_size.as_str().to_string();
    let bars_owned: Vec<HistoricalBar> = bars.to_vec();

    db.with_conn(move |conn| {
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT OR REPLACE INTO bars_cache \
                 (symbol, bar_size, bar_time, open, high, low, close, volume, wap) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            )?;
            for bar in &bars_owned {
                let ts = parse_ibkr_time(&bar.time).map_err(|e| {
                    StorageError::Migration(format!("bad bar time '{}': {e}", bar.time))
                })?;
                stmt.execute(rusqlite::params![
                    symbol,
                    bar_size_str,
                    ts,
                    bar.open,
                    bar.high,
                    bar.low,
                    bar.close,
                    bar.volume,
                    bar.wap,
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    })
    .await
    .map_err(map_storage_err)
}

fn map_storage_err(e: StorageError) -> IbkrError {
    IbkrError::RequestFailed(format!("storage: {e}"))
}

/// Render a unix-second timestamp back into the IBKR string format.
/// Daily bars are rendered as `YYYYMMDD`; non-midnight as `YYYYMMDD HH:MM:SS`.
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
