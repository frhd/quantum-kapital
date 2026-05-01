//! `social_sentiment` table read/write helpers.
//!
//! Synchronous closures handed to [`Db::with_conn`]; the rest of the
//! module stays at the row-mapper level. All inserts go through this
//! module so the schema stays in one place.

use std::sync::Arc;

use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::services::social_sentiment::types::{SentimentSample, SocialSentimentRow};
use crate::storage::{Db, Result as StorageResult};

fn map_row(row: &Row<'_>) -> rusqlite::Result<SocialSentimentRow> {
    Ok(SocialSentimentRow {
        id: row.get("id")?,
        source: row.get("source")?,
        symbol: row.get("symbol")?,
        score: row.get("score")?,
        mentions_24h: row.get("mentions_24h")?,
        sentiment_label: row.get("sentiment_label")?,
        rank: row.get("rank")?,
        raw_payload: row.get("raw_payload")?,
        is_stale: {
            let v: i64 = row.get("is_stale")?;
            v != 0
        },
        fetched_at: row.get("fetched_at")?,
    })
}

#[allow(dead_code)] // exercised through `insert_sample` + tests
fn insert_sample_sync(
    conn: &mut Connection,
    sample: &SentimentSample,
    fetched_at: i64,
) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO social_sentiment \
         (source, symbol, score, mentions_24h, sentiment_label, rank, \
          raw_payload, is_stale, fetched_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            sample.source.as_str(),
            sample.symbol.to_uppercase(),
            sample.score,
            sample.mentions_24h,
            sample.label.as_ref().map(|l| l.as_str()),
            sample.rank,
            sample.raw_payload,
            if sample.is_stale { 1_i64 } else { 0_i64 },
            fetched_at,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Persist one [`SentimentSample`]. `fetched_at` is the wall-clock
/// timestamp the caller wants to record (so callers with a `Clock`
/// seam can pin it deterministically in tests).
#[allow(dead_code)] // exercised by per-tool tests + integration paths
pub async fn insert_sample(
    db: Arc<Db>,
    sample: SentimentSample,
    fetched_at: i64,
) -> StorageResult<i64> {
    db.with_conn(move |conn| {
        let id = insert_sample_sync(conn, &sample, fetched_at)?;
        Ok(id)
    })
    .await
}

/// Bulk-insert in a single transaction. Returns the number of rows
/// written. Used by the orchestrator after a fan-out so a single tick
/// is one transaction (and a single fsync) regardless of how many
/// providers responded.
pub async fn insert_samples(
    db: Arc<Db>,
    samples: Vec<SentimentSample>,
    fetched_at: i64,
) -> StorageResult<usize> {
    db.with_conn(move |conn| {
        let tx = conn.transaction()?;
        let mut count = 0_usize;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO social_sentiment \
                 (source, symbol, score, mentions_24h, sentiment_label, rank, \
                  raw_payload, is_stale, fetched_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            )?;
            for sample in &samples {
                stmt.execute(params![
                    sample.source.as_str(),
                    sample.symbol.to_uppercase(),
                    sample.score,
                    sample.mentions_24h,
                    sample.label.as_ref().map(|l| l.as_str()),
                    sample.rank,
                    sample.raw_payload,
                    if sample.is_stale { 1_i64 } else { 0_i64 },
                    fetched_at,
                ])?;
                count += 1;
            }
        }
        tx.commit()?;
        Ok(count)
    })
    .await
}

/// All rows for `symbol` with `fetched_at >= since`, newest first.
/// Optionally filtered to a subset of sources.
pub async fn rows_for_symbol_since(
    db: Arc<Db>,
    symbol: String,
    since: i64,
    sources: Option<Vec<String>>,
) -> StorageResult<Vec<SocialSentimentRow>> {
    let symbol_upper = symbol.to_uppercase();
    db.with_conn(move |conn| {
        let mut sql = String::from(
            "SELECT id, source, symbol, score, mentions_24h, sentiment_label, rank, \
             raw_payload, is_stale, fetched_at \
             FROM social_sentiment \
             WHERE symbol = ?1 AND fetched_at >= ?2",
        );
        if let Some(srcs) = &sources {
            if !srcs.is_empty() {
                let placeholders: Vec<String> =
                    (3..3 + srcs.len()).map(|i| format!("?{i}")).collect();
                sql.push_str(&format!(" AND source IN ({})", placeholders.join(",")));
            }
        }
        sql.push_str(" ORDER BY fetched_at DESC");

        let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = vec![
            Box::new(symbol_upper.clone()),
            Box::new(since),
        ];
        if let Some(srcs) = sources {
            for s in srcs {
                params_vec.push(Box::new(s));
            }
        }
        let params_refs: Vec<&dyn rusqlite::ToSql> =
            params_vec.iter().map(|b| b.as_ref()).collect();

        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params_refs.as_slice(), map_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    })
    .await
}

/// Latest row per source for `symbol`. Used by the UI widget to render
/// the "one row, three sources" snapshot.
pub async fn latest_per_source(
    db: Arc<Db>,
    symbol: String,
) -> StorageResult<Vec<SocialSentimentRow>> {
    let symbol_upper = symbol.to_uppercase();
    db.with_conn(move |conn| {
        // Per-source `MAX(fetched_at)` self-join. Cheap on the existing
        // `(symbol, source, fetched_at DESC)` index.
        let mut stmt = conn.prepare(
            "SELECT s.id, s.source, s.symbol, s.score, s.mentions_24h, s.sentiment_label, \
                    s.rank, s.raw_payload, s.is_stale, s.fetched_at \
             FROM social_sentiment s \
             INNER JOIN ( \
                 SELECT source, MAX(fetched_at) AS mx \
                 FROM social_sentiment \
                 WHERE symbol = ?1 \
                 GROUP BY source \
             ) m ON m.source = s.source AND m.mx = s.fetched_at \
             WHERE s.symbol = ?1 \
             ORDER BY s.source",
        )?;
        let rows = stmt
            .query_map([&symbol_upper], map_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    })
    .await
}

/// Latest row from a single source — convenience used by tests.
#[allow(dead_code)]
pub async fn latest_for_source(
    db: Arc<Db>,
    symbol: String,
    source: String,
) -> StorageResult<Option<SocialSentimentRow>> {
    let symbol_upper = symbol.to_uppercase();
    db.with_conn(move |conn| {
        let row = conn
            .query_row(
                "SELECT id, source, symbol, score, mentions_24h, sentiment_label, rank, \
                 raw_payload, is_stale, fetched_at \
                 FROM social_sentiment \
                 WHERE symbol = ?1 AND source = ?2 \
                 ORDER BY fetched_at DESC LIMIT 1",
                params![symbol_upper, source],
                map_row,
            )
            .optional()?;
        Ok(row)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::social_sentiment::types::{SentimentLabel, SentimentSource};
    use tempfile::NamedTempFile;

    fn make_db() -> (NamedTempFile, Arc<Db>) {
        let tmp = NamedTempFile::new().expect("tempfile");
        let db = Db::open(tmp.path()).expect("open db");
        (tmp, Arc::new(db))
    }

    fn sample(
        source: SentimentSource,
        symbol: &str,
        score: Option<f64>,
        label: Option<SentimentLabel>,
    ) -> SentimentSample {
        SentimentSample {
            source,
            symbol: symbol.to_string(),
            score,
            mentions_24h: Some(42),
            label,
            rank: None,
            raw_payload: "{}".to_string(),
            is_stale: false,
        }
    }

    #[tokio::test]
    async fn insert_then_latest_round_trips() {
        let (_tmp, db) = make_db();
        let now = 1_700_000_000;
        let id = insert_sample(
            Arc::clone(&db),
            sample(SentimentSource::Apewisdom, "TSLA", Some(0.5), Some(SentimentLabel::Bullish)),
            now,
        )
        .await
        .unwrap();
        assert!(id > 0);

        let row = latest_for_source(Arc::clone(&db), "TSLA".into(), "apewisdom".into())
            .await
            .unwrap()
            .expect("row present");
        assert_eq!(row.symbol, "TSLA");
        assert_eq!(row.source, "apewisdom");
        assert_eq!(row.score, Some(0.5));
        assert_eq!(row.sentiment_label.as_deref(), Some("bullish"));
        assert_eq!(row.fetched_at, now);
    }

    #[tokio::test]
    async fn rows_for_symbol_since_filters_by_window_and_source() {
        let (_tmp, db) = make_db();
        let now = 1_700_000_000_i64;
        // Old, in-window apewisdom + recent stocktwits + ancient (filtered)
        insert_sample(
            Arc::clone(&db),
            sample(SentimentSource::Apewisdom, "TSLA", Some(0.2), None),
            now - 3600,
        )
        .await
        .unwrap();
        insert_sample(
            Arc::clone(&db),
            sample(SentimentSource::Stocktwits, "TSLA", Some(-0.1), None),
            now - 60,
        )
        .await
        .unwrap();
        insert_sample(
            Arc::clone(&db),
            sample(SentimentSource::Apewisdom, "TSLA", Some(0.3), None),
            now - 86_400 * 3, // outside the 24h window
        )
        .await
        .unwrap();

        let in_window = rows_for_symbol_since(
            Arc::clone(&db),
            "TSLA".into(),
            now - 86_400,
            None,
        )
        .await
        .unwrap();
        assert_eq!(in_window.len(), 2);
        assert!(in_window[0].fetched_at >= in_window[1].fetched_at, "newest first");

        let only_stocktwits = rows_for_symbol_since(
            Arc::clone(&db),
            "TSLA".into(),
            now - 86_400,
            Some(vec!["stocktwits".into()]),
        )
        .await
        .unwrap();
        assert_eq!(only_stocktwits.len(), 1);
        assert_eq!(only_stocktwits[0].source, "stocktwits");
    }

    #[tokio::test]
    async fn latest_per_source_returns_one_row_per_source() {
        let (_tmp, db) = make_db();
        let now = 1_700_000_000_i64;
        // Two apewisdom rows (older + newer), one stocktwits.
        for (src, ts, score) in [
            (SentimentSource::Apewisdom, now - 7200, 0.1),
            (SentimentSource::Apewisdom, now - 60, 0.7),
            (SentimentSource::Stocktwits, now - 30, -0.2),
        ] {
            insert_sample(
                Arc::clone(&db),
                sample(src, "TSLA", Some(score), None),
                ts,
            )
            .await
            .unwrap();
        }

        let latest = latest_per_source(Arc::clone(&db), "TSLA".into())
            .await
            .unwrap();
        assert_eq!(latest.len(), 2, "one row per source");
        let ape = latest
            .iter()
            .find(|r| r.source == "apewisdom")
            .expect("apewisdom row");
        assert_eq!(ape.score, Some(0.7), "newest apewisdom value wins");
        let st = latest
            .iter()
            .find(|r| r.source == "stocktwits")
            .expect("stocktwits row");
        assert_eq!(st.score, Some(-0.2));
    }

    #[tokio::test]
    async fn insert_samples_writes_all_rows_in_one_transaction() {
        let (_tmp, db) = make_db();
        let now = 1_700_000_500_i64;
        let written = insert_samples(
            Arc::clone(&db),
            vec![
                sample(SentimentSource::Apewisdom, "tsla", Some(0.4), None),
                sample(SentimentSource::Stocktwits, "TSLA", Some(0.1), None),
                sample(SentimentSource::RedditWsb, "tsla", None, None),
            ],
            now,
        )
        .await
        .unwrap();
        assert_eq!(written, 3);

        let latest = latest_per_source(Arc::clone(&db), "TSLA".into())
            .await
            .unwrap();
        assert_eq!(latest.len(), 3, "three sources persisted, symbol upper-cased");
    }
}
