//! `candidate_universe` table read/write helpers.
//!
//! Single source of truth for the schema; the rest of the module only
//! deals in [`Candidate`] / [`NewCandidate`]. Upsert merges per-source
//! provenance into the `sources` JSON array — a second hit from the
//! same `source` overwrites that entry's score/rank/meta but keeps the
//! row's `first_seen` immutable.

#![allow(dead_code)] // consumers (promoter, MCP tools, scheduler) land in subsequent steps

use std::sync::Arc;

use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::services::candidate_universe::types::{
    Candidate, CandidateFilter, CandidateSource, NewCandidate,
};
use crate::storage::{Db, Result as StorageResult, StorageError};

fn map_row(row: &Row<'_>) -> rusqlite::Result<Candidate> {
    let sources_json: String = row.get("sources")?;
    let sources: Vec<CandidateSource> = serde_json::from_str(&sources_json).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::other(format!(
                "candidate_universe.sources JSON decode: {e}"
            ))),
        )
    })?;
    Ok(Candidate {
        symbol: row.get("symbol")?,
        score: row.get("score")?,
        sources,
        reason_md: row.get("reason_md")?,
        first_seen: row.get("first_seen")?,
        last_seen: row.get("last_seen")?,
        decay_at: row.get("decay_at")?,
        promoted_at: row.get("promoted_at")?,
    })
}

fn merge_sources(existing: &[CandidateSource], incoming: CandidateSource) -> Vec<CandidateSource> {
    let mut out: Vec<CandidateSource> = existing
        .iter()
        .filter(|s| s.source != incoming.source)
        .cloned()
        .collect();
    out.push(incoming);
    out
}

fn merged_score(sources: &[CandidateSource]) -> f64 {
    sources
        .iter()
        .map(|s| s.score)
        .fold(0.0_f64, |acc, s| acc.max(s))
}

/// Insert or merge a candidate. `now` is the wall-clock seconds the
/// caller wants stamped on `last_seen` (and on `first_seen` when the
/// row is new). Returns the post-write [`Candidate`] so the caller has
/// the merged score + sources without a follow-up read.
pub async fn upsert(db: Arc<Db>, new: NewCandidate, now: i64) -> StorageResult<Candidate> {
    let symbol = new.symbol.to_uppercase();
    let mut incoming = new.source;
    incoming.last_seen = now;
    let new_decay_at = now.saturating_add(new.ttl_seconds.max(0));
    let reason = new.reason_md;
    db.with_conn(move |conn| {
        let existing = read_existing(conn, &symbol)?;
        let (merged_sources_vec, first_seen, decay_at, promoted_at) = match existing {
            Some(curr) => {
                let merged = merge_sources(&curr.sources, incoming);
                // Decay extends to whichever is later — keeps a hot
                // signal from being kicked out by a stale one.
                let decay = curr.decay_at.max(new_decay_at);
                (merged, curr.first_seen, decay, curr.promoted_at)
            }
            None => (vec![incoming], now, new_decay_at, None),
        };
        let score = merged_score(&merged_sources_vec);
        let sources_json = serde_json::to_string(&merged_sources_vec).map_err(|e| {
            StorageError::Migration(format!("encode candidate sources: {e}"))
        })?;
        conn.execute(
            "INSERT INTO candidate_universe \
                 (symbol, score, sources, reason_md, first_seen, last_seen, decay_at, promoted_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) \
             ON CONFLICT(symbol) DO UPDATE SET \
                 score      = excluded.score, \
                 sources    = excluded.sources, \
                 reason_md  = COALESCE(excluded.reason_md, candidate_universe.reason_md), \
                 last_seen  = excluded.last_seen, \
                 decay_at   = excluded.decay_at",
            params![
                symbol,
                score,
                sources_json,
                reason,
                first_seen,
                now,
                decay_at,
                promoted_at,
            ],
        )?;
        Ok(Candidate {
            symbol,
            score,
            sources: merged_sources_vec,
            reason_md: reason,
            first_seen,
            last_seen: now,
            decay_at,
            promoted_at,
        })
    })
    .await
}

fn read_existing(conn: &Connection, symbol: &str) -> StorageResult<Option<Candidate>> {
    let row = conn
        .query_row(
            "SELECT symbol, score, sources, reason_md, first_seen, last_seen, decay_at, promoted_at \
             FROM candidate_universe WHERE symbol = ?1",
            params![symbol],
            map_row,
        )
        .optional()?;
    Ok(row)
}

/// Single-row lookup. Returns `None` for unknown symbols.
#[allow(dead_code)] // consumed by `promote_candidate` MCP tool + Tauri commands
pub async fn get(db: Arc<Db>, symbol: String) -> StorageResult<Option<Candidate>> {
    let symbol_upper = symbol.to_uppercase();
    db.with_conn(move |conn| read_existing(conn, &symbol_upper)).await
}

/// List candidates matching `filter`. Default ordering is `score DESC`
/// then `last_seen DESC` so the agent inbox surfaces hottest-and-freshest
/// first.
pub async fn list(db: Arc<Db>, filter: CandidateFilter) -> StorageResult<Vec<Candidate>> {
    let limit = filter.limit.unwrap_or(100).min(1_000);
    db.with_conn(move |conn| {
        let mut sql = String::from(
            "SELECT symbol, score, sources, reason_md, first_seen, last_seen, decay_at, promoted_at \
             FROM candidate_universe WHERE 1=1",
        );
        let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if !filter.include_promoted {
            sql.push_str(" AND promoted_at IS NULL");
        }
        if let Some(min) = filter.min_score {
            params_vec.push(Box::new(min));
            sql.push_str(&format!(" AND score >= ?{}", params_vec.len()));
        }
        if let Some(since) = filter.since_last_seen {
            params_vec.push(Box::new(since));
            sql.push_str(&format!(" AND last_seen >= ?{}", params_vec.len()));
        }
        if let Some(needle) = filter.source_substring.as_ref() {
            // SQLite has no first-class JSON contains — match against
            // the raw text. Lowercase both sides so the agent doesn't
            // have to know our internal source-id casing.
            params_vec.push(Box::new(format!("%{}%", needle.to_lowercase())));
            sql.push_str(&format!(" AND lower(sources) LIKE ?{}", params_vec.len()));
        }
        sql.push_str(" ORDER BY score DESC, last_seen DESC LIMIT ");
        sql.push_str(&limit.to_string());
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

/// Stamp `promoted_at = now` for `symbol`. Returns the post-update row,
/// or `None` if no candidate exists with that symbol. Re-promoting an
/// already-promoted row is a no-op (`promoted_at` is preserved).
pub async fn mark_promoted(
    db: Arc<Db>,
    symbol: String,
    now: i64,
) -> StorageResult<Option<Candidate>> {
    let symbol_upper = symbol.to_uppercase();
    db.with_conn(move |conn| {
        let updated = conn.execute(
            "UPDATE candidate_universe \
                 SET promoted_at = COALESCE(promoted_at, ?1) \
                 WHERE symbol = ?2",
            params![now, symbol_upper],
        )?;
        if updated == 0 {
            return Ok(None);
        }
        read_existing(conn, &symbol_upper)
    })
    .await
}

/// Delete unpromoted rows whose decay deadline has passed. Returns the
/// number of rows evicted. Promoted rows are kept — see V05 docs.
pub async fn delete_expired(db: Arc<Db>, now: i64) -> StorageResult<usize> {
    db.with_conn(move |conn| {
        let n = conn.execute(
            "DELETE FROM candidate_universe \
                 WHERE promoted_at IS NULL AND decay_at <= ?1",
            params![now],
        )?;
        Ok(n)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::NamedTempFile;

    fn make_db() -> (NamedTempFile, Arc<Db>) {
        let tmp = NamedTempFile::new().expect("tempfile");
        let db = Db::open(tmp.path()).expect("open db");
        (tmp, Arc::new(db))
    }

    fn src(name: &str, score: f64) -> CandidateSource {
        CandidateSource {
            source: name.to_string(),
            score,
            rank: None,
            meta: json!({}),
            last_seen: 0,
        }
    }

    fn new_candidate(symbol: &str, source: CandidateSource, ttl: i64) -> NewCandidate {
        NewCandidate {
            symbol: symbol.to_string(),
            source,
            reason_md: Some(format!("via {}", symbol)),
            ttl_seconds: ttl,
        }
    }

    #[tokio::test]
    async fn upsert_inserts_new_row_with_normalised_symbol() {
        let (_tmp, db) = make_db();
        let now = 1_700_000_000_i64;

        let saved = upsert(
            Arc::clone(&db),
            new_candidate("tsla", src("scanner_top_perc_gain", 0.7), 7 * 86_400),
            now,
        )
        .await
        .unwrap();

        assert_eq!(saved.symbol, "TSLA");
        assert_eq!(saved.score, 0.7);
        assert_eq!(saved.first_seen, now);
        assert_eq!(saved.last_seen, now);
        assert_eq!(saved.decay_at, now + 7 * 86_400);
        assert!(saved.promoted_at.is_none());
        assert_eq!(saved.sources.len(), 1);
        assert_eq!(saved.sources[0].source, "scanner_top_perc_gain");
        assert_eq!(saved.sources[0].last_seen, now);
    }

    #[tokio::test]
    async fn upsert_merges_sources_taking_max_score_and_extending_decay() {
        let (_tmp, db) = make_db();
        let t0 = 1_700_000_000_i64;
        upsert(
            Arc::clone(&db),
            new_candidate("TSLA", src("scanner_top_perc_gain", 0.5), 86_400),
            t0,
        )
        .await
        .unwrap();

        // Second source with higher score and longer ttl arrives later.
        let t1 = t0 + 3_600;
        let merged = upsert(
            Arc::clone(&db),
            new_candidate("TSLA", src("sentiment_surge", 0.8), 2 * 86_400),
            t1,
        )
        .await
        .unwrap();

        assert_eq!(merged.score, 0.8, "MAX(0.5, 0.8)");
        assert_eq!(merged.first_seen, t0, "first_seen immutable");
        assert_eq!(merged.last_seen, t1);
        assert_eq!(merged.decay_at, t1 + 2 * 86_400);
        let names: Vec<_> = merged.sources.iter().map(|s| s.source.as_str()).collect();
        assert!(names.contains(&"scanner_top_perc_gain"));
        assert!(names.contains(&"sentiment_surge"));
    }

    #[tokio::test]
    async fn upsert_replaces_same_source_entry() {
        let (_tmp, db) = make_db();
        let t0 = 1_700_000_000_i64;
        upsert(
            Arc::clone(&db),
            new_candidate("TSLA", src("scanner_top_perc_gain", 0.4), 86_400),
            t0,
        )
        .await
        .unwrap();
        let again = upsert(
            Arc::clone(&db),
            new_candidate("TSLA", src("scanner_top_perc_gain", 0.9), 86_400),
            t0 + 60,
        )
        .await
        .unwrap();
        assert_eq!(again.sources.len(), 1, "same source replaces, not appends");
        assert_eq!(again.sources[0].score, 0.9);
        assert_eq!(again.score, 0.9);
    }

    #[tokio::test]
    async fn list_orders_by_score_desc_and_filters_promoted() {
        let (_tmp, db) = make_db();
        let now = 1_700_000_000_i64;
        for (sym, sc) in [("AAA", 0.2), ("BBB", 0.9), ("CCC", 0.5)] {
            upsert(
                Arc::clone(&db),
                new_candidate(sym, src("scanner_top_perc_gain", sc), 86_400),
                now,
            )
            .await
            .unwrap();
        }
        // Promote BBB.
        mark_promoted(Arc::clone(&db), "BBB".into(), now).await.unwrap();

        let unpromoted = list(Arc::clone(&db), CandidateFilter::default())
            .await
            .unwrap();
        let symbols: Vec<_> = unpromoted.iter().map(|c| c.symbol.as_str()).collect();
        assert_eq!(symbols, vec!["CCC", "AAA"], "promoted hidden, score-desc");

        let with_promoted = list(
            Arc::clone(&db),
            CandidateFilter {
                include_promoted: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(with_promoted.len(), 3);
    }

    #[tokio::test]
    async fn list_filters_by_source_substring_and_min_score() {
        let (_tmp, db) = make_db();
        let now = 1_700_000_000_i64;
        upsert(
            Arc::clone(&db),
            new_candidate("AAA", src("scanner_top_perc_gain", 0.3), 86_400),
            now,
        )
        .await
        .unwrap();
        upsert(
            Arc::clone(&db),
            new_candidate("BBB", src("sentiment_surge", 0.6), 86_400),
            now,
        )
        .await
        .unwrap();

        let only_sentiment = list(
            Arc::clone(&db),
            CandidateFilter {
                source_substring: Some("sentiment".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(only_sentiment.len(), 1);
        assert_eq!(only_sentiment[0].symbol, "BBB");

        let high_only = list(
            Arc::clone(&db),
            CandidateFilter {
                min_score: Some(0.5),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(high_only.len(), 1);
        assert_eq!(high_only[0].symbol, "BBB");
    }

    #[tokio::test]
    async fn mark_promoted_is_idempotent_and_returns_none_for_missing() {
        let (_tmp, db) = make_db();
        let now = 1_700_000_000_i64;
        upsert(
            Arc::clone(&db),
            new_candidate("AAA", src("scanner_top_perc_gain", 0.5), 86_400),
            now,
        )
        .await
        .unwrap();
        let first = mark_promoted(Arc::clone(&db), "AAA".into(), now).await.unwrap();
        assert!(first.unwrap().promoted_at.is_some());
        // Idempotent: a re-call doesn't change `promoted_at`.
        let second = mark_promoted(Arc::clone(&db), "AAA".into(), now + 99).await.unwrap();
        assert_eq!(second.unwrap().promoted_at, Some(now));

        let missing = mark_promoted(Arc::clone(&db), "ZZZ".into(), now).await.unwrap();
        assert!(missing.is_none());
    }

    #[tokio::test]
    async fn delete_expired_evicts_unpromoted_only() {
        let (_tmp, db) = make_db();
        let t0 = 1_700_000_000_i64;
        // Two unpromoted (one stale, one fresh) + one promoted-but-stale.
        upsert(
            Arc::clone(&db),
            new_candidate("STALE", src("scanner_top_perc_gain", 0.5), 60),
            t0,
        )
        .await
        .unwrap();
        upsert(
            Arc::clone(&db),
            new_candidate("FRESH", src("scanner_top_perc_gain", 0.5), 86_400),
            t0,
        )
        .await
        .unwrap();
        upsert(
            Arc::clone(&db),
            new_candidate("KEPT", src("scanner_top_perc_gain", 0.5), 60),
            t0,
        )
        .await
        .unwrap();
        mark_promoted(Arc::clone(&db), "KEPT".into(), t0).await.unwrap();

        let evicted = delete_expired(Arc::clone(&db), t0 + 120).await.unwrap();
        assert_eq!(evicted, 1);

        let remaining = list(
            Arc::clone(&db),
            CandidateFilter {
                include_promoted: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let symbols: Vec<_> = remaining.iter().map(|c| c.symbol.as_str()).collect();
        assert!(symbols.contains(&"FRESH"));
        assert!(symbols.contains(&"KEPT"));
        assert!(!symbols.contains(&"STALE"));
    }
}
