//! Vendor-neutral SQLite cache for `NewsItem` payloads + LLM
//! `NewsVerdict`s, keyed by symbol. Producers (today: `IbkrNewsProvider`)
//! call [`write_cache`] after a successful fetch; consumers
//! (`NewsInterpreter`, the MCP `get_news` tool) read through
//! [`read_cache_with_verdict`] and the interpreter writes its verdict
//! back via [`write_verdict`].
//!
//! Replaces the AV-shaped helpers that used to live in
//! `services/financial_data_service/news.rs` (Phase 8 deletion). The
//! schema is unchanged — `news_cache` is the same SQLite table.

use crate::ibkr::types::news::NewsItem;
use crate::storage::Db;

/// Cached news row, including the optional LLM-derived verdict JSON.
#[derive(Debug, Clone)]
pub struct CachedNews {
    /// Unix seconds at which the payload landed in cache. Carried along
    /// the read path so callers can age the verdict.
    #[allow(dead_code)]
    pub fetched_at: i64,
    pub items: Vec<NewsItem>,
    /// Raw JSON of [`crate::ibkr::types::NewsVerdict`]. `None` when the
    /// interpreter has not yet run for the current payload (every fresh
    /// [`write_cache`] call clears the column).
    pub verdict_json: Option<String>,
}

/// Read the cached news payload + LLM verdict for `symbol`. Returns
/// `Ok(None)` when no row exists. The verdict column is `NULL` until
/// the news interpreter populates it.
pub async fn read_cache_with_verdict(
    db: &Db,
    symbol: &str,
) -> Result<Option<CachedNews>, Box<dyn std::error::Error + Send + Sync>> {
    let symbol = symbol.to_string();
    let row = db
        .with_conn(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT fetched_at, payload, news_verdict_json FROM news_cache WHERE symbol = ?1",
            )?;
            let mut rows = stmt.query(rusqlite::params![symbol])?;
            if let Some(row) = rows.next()? {
                let fetched_at: i64 = row.get(0)?;
                let payload: String = row.get(1)?;
                let verdict_json: Option<String> = row.get(2)?;
                Ok(Some((fetched_at, payload, verdict_json)))
            } else {
                Ok(None)
            }
        })
        .await?;

    match row {
        Some((fetched_at, payload, verdict_json)) => {
            let items: Vec<NewsItem> = serde_json::from_str(&payload)?;
            Ok(Some(CachedNews {
                fetched_at,
                items,
                verdict_json,
            }))
        }
        None => Ok(None),
    }
}

/// Persist the LLM verdict for `symbol`. No-ops cleanly when no
/// `news_cache` row exists (the interpreter only runs after the
/// producer has written one). `verdict_json` is stored verbatim.
pub async fn write_verdict(
    db: &Db,
    symbol: &str,
    verdict_json: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let symbol = symbol.to_string();
    let verdict_json = verdict_json.to_string();
    db.with_conn(move |conn| {
        conn.execute(
            "UPDATE news_cache SET news_verdict_json = ?2 WHERE symbol = ?1",
            rusqlite::params![symbol, verdict_json],
        )?;
        Ok(())
    })
    .await?;
    Ok(())
}

/// Persist a fresh news payload for `symbol`. `INSERT OR REPLACE`
/// drops the existing row, which clears the `news_verdict_json`
/// column — a new payload invalidates any prior verdict and the
/// interpreter must re-run.
pub async fn write_cache(
    db: &Db,
    symbol: &str,
    fetched_at: i64,
    items: &[NewsItem],
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let symbol = symbol.to_string();
    let payload = serde_json::to_string(items)?;
    db.with_conn(move |conn| {
        conn.execute(
            "INSERT OR REPLACE INTO news_cache (symbol, fetched_at, payload) \
             VALUES (?1, ?2, ?3)",
            rusqlite::params![symbol, fetched_at, payload],
        )?;
        Ok(())
    })
    .await?;
    Ok(())
}
