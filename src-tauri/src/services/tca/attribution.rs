//! Phase 2 — per-strategy attribution + slippage distribution.
//!
//! Read-only queries over the linkage columns added in V18. Two
//! queries:
//!
//! 1. `attribution(date_from, date_to, account)` — one row per
//!    detector class plus an "unattributed" bucket. Numbers come
//!    from `executions` joined to `setups` for `strategy`. NULL
//!    `setup_id` ⇒ unattributed.
//! 2. `slippage_distribution(date_from, date_to, account, edges)` —
//!    histogram bucketed by user-supplied edges, keyed by strategy
//!    + a coarse symbol-liquidity bucket (placeholder = "all" until
//!    a real classifier lands).
//!
//! All money is integer cents on read, no f64 round-trip drift.

use std::sync::Arc;

use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use chrono_tz::America::New_York;
use rusqlite::params;

use crate::storage::error::Result as StorageResult;
use crate::storage::Db;

use super::types::{
    default_histogram_edges, AttributionRow, SlippageBucket, SlippageDistributionRow,
};

#[derive(Clone)]
pub struct AttributionService {
    db: Arc<Db>,
}

impl AttributionService {
    pub fn new(db: Arc<Db>) -> Self {
        Self { db }
    }

    /// One row per strategy + an `unattributed` bucket. The
    /// `pnl_cents` columns convert IBKR-reported `realized_pnl` and
    /// `commission` (both REAL dollars) to cents at read time.
    /// Opening-leg fills (`realized_pnl IS NULL`) contribute to
    /// `n_trades` but not to `realized_pnl_cents`.
    pub async fn attribution(
        &self,
        date_from: NaiveDate,
        date_to_inclusive: NaiveDate,
        account: &str,
    ) -> StorageResult<Vec<AttributionRow>> {
        let (start, end) = et_range_utc(date_from, date_to_inclusive);
        let account = account.to_string();
        self.db
            .with_conn(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT
                        s.strategy AS strategy,
                        COUNT(*) AS n_trades,
                        CAST(ROUND(COALESCE(SUM(e.realized_pnl), 0) * 100) AS INTEGER) AS gross_pnl_cents,
                        CAST(ROUND(
                            COALESCE(SUM(e.realized_pnl), 0) * 100
                          - COALESCE(SUM(e.commission), 0) * 100
                        ) AS INTEGER) AS net_pnl_cents,
                        COALESCE(AVG(e.slippage_bps), 0) AS avg_slip_bps,
                        SUM(CASE WHEN e.slippage_bps IS NOT NULL THEN 1 ELSE 0 END) AS n_with_slip,
                        CAST(ROUND(COALESCE(SUM(e.realized_pnl), 0) * 100) AS INTEGER) AS realized_pnl_cents
                     FROM executions e
                     LEFT JOIN setups s ON s.id = e.setup_id
                     WHERE e.account = ?1
                       AND e.exec_time >= ?2 AND e.exec_time < ?3
                     GROUP BY s.strategy
                     ORDER BY n_trades DESC, s.strategy",
                )?;
                let rows = stmt
                    .query_map(
                        params![account, start.to_rfc3339(), end.to_rfc3339()],
                        |row| {
                            Ok(AttributionRow {
                                strategy: row.get::<_, Option<String>>(0)?,
                                n_trades: row.get(1)?,
                                gross_pnl_cents: row.get(2)?,
                                net_pnl_cents: row.get(3)?,
                                avg_slippage_bps: row.get(4)?,
                                n_with_slippage: row.get(5)?,
                                realized_pnl_cents: row.get(6)?,
                            })
                        },
                    )?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await
    }

    /// Slippage histogram. v1 emits a single `liquidity_bucket="all"`
    /// per strategy — the per-strategy split is the load-bearing
    /// signal, and a real liquidity classifier is a P6 concern. The
    /// query is shaped so a future "join symbols → liquidity_bucket
    /// table" lift happens without changing the wire DTO.
    pub async fn slippage_distribution(
        &self,
        date_from: NaiveDate,
        date_to_inclusive: NaiveDate,
        account: &str,
        edges: Option<Vec<(i64, i64)>>,
    ) -> StorageResult<Vec<SlippageDistributionRow>> {
        let edges = edges.unwrap_or_else(default_histogram_edges);
        let (start, end) = et_range_utc(date_from, date_to_inclusive);
        let account = account.to_string();
        self.db
            .with_conn(move |conn| {
                #[derive(Default)]
                struct Counts {
                    by_strategy: std::collections::BTreeMap<Option<String>, Vec<i64>>,
                }
                let mut counts = Counts::default();
                let n_buckets = edges.len();
                let mut stmt = conn.prepare(
                    "SELECT s.strategy AS strategy, e.slippage_bps AS bps
                     FROM executions e
                     LEFT JOIN setups s ON s.id = e.setup_id
                     WHERE e.account = ?1
                       AND e.exec_time >= ?2 AND e.exec_time < ?3
                       AND e.slippage_bps IS NOT NULL",
                )?;
                let mut rows =
                    stmt.query(params![account, start.to_rfc3339(), end.to_rfc3339()])?;
                while let Some(row) = rows.next()? {
                    let strategy: Option<String> = row.get(0)?;
                    let bps: i64 = row.get(1)?;
                    let entry = counts
                        .by_strategy
                        .entry(strategy)
                        .or_insert_with(|| vec![0; n_buckets]);
                    if let Some(idx) = bucket_index(bps, &edges) {
                        entry[idx] += 1;
                    }
                }
                let out = counts
                    .by_strategy
                    .into_iter()
                    .map(|(strategy, ns)| SlippageDistributionRow {
                        strategy,
                        liquidity_bucket: "all".to_string(),
                        buckets: edges
                            .iter()
                            .zip(ns.iter())
                            .map(|((lo, hi), n)| SlippageBucket {
                                lower_bps: *lo,
                                upper_bps: *hi,
                                n: *n,
                            })
                            .collect(),
                    })
                    .collect::<Vec<_>>();
                Ok(out)
            })
            .await
    }
}

fn bucket_index(bps: i64, edges: &[(i64, i64)]) -> Option<usize> {
    edges
        .iter()
        .position(|(lo, hi)| bps >= *lo && (bps < *hi || *hi == i64::MAX))
}

/// Convert a `[date_from, date_to_inclusive]` ET range to a
/// `[start_utc, end_utc)` half-open window. Matches the executions
/// store's day-bounds convention.
fn et_range_utc(from: NaiveDate, to_inclusive: NaiveDate) -> (DateTime<Utc>, DateTime<Utc>) {
    let day_start_naive = from.and_hms_opt(0, 0, 0).expect("midnight valid");
    let next_day_naive = (to_inclusive + chrono::Duration::days(1))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bucket_index_includes_lower_excludes_upper() {
        let edges = vec![(0, 5), (5, 10), (10, i64::MAX)];
        assert_eq!(bucket_index(0, &edges), Some(0));
        assert_eq!(bucket_index(4, &edges), Some(0));
        assert_eq!(bucket_index(5, &edges), Some(1));
        assert_eq!(bucket_index(9, &edges), Some(1));
        assert_eq!(bucket_index(10, &edges), Some(2));
        assert_eq!(bucket_index(i64::MAX, &edges), Some(2));
    }
}
