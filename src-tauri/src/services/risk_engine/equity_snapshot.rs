//! Phase 1 — equity snapshot fetch + cache.
//!
//! `EquitySnapshotService` owns the contract that one trading day
//! gets exactly one NLV row per account in `equity_snapshots`. The
//! engine's sizing decisions pin to this row so two consecutive
//! setups read the same equity and produce comparable dollar-risk.
//!
//! Fetcher is a trait so tests don't have to construct a live IBKR
//! client; production wiring blanket-impls over `IbkrClientTrait`.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::America::New_York;
use rusqlite::OptionalExtension;
use thiserror::Error;
use tracing::warn;

use crate::ibkr::client::IbkrClient;
use crate::ibkr::error::IbkrError;
use crate::storage::error::StorageError;
use crate::storage::Db;

use super::types::{EquitySnapshot, EquitySource};

/// IBKR account-summary tag carrying NetLiquidation in the live
/// account. Our `IbkrClientTrait::get_account_summary` returns rows
/// where this is the canonical NLV indicator.
const NLV_TAG: &str = "NetLiquidation";

/// Trait seam for the live equity fetch. Production: blanket impl
/// over `IbkrClientTrait`. Tests: hand-rolled stubs without the IBKR
/// stack. Returns NLV in dollars; the service converts to cents.
#[async_trait]
pub trait EquityFetcher: Send + Sync {
    async fn fetch_nlv(&self, account: &str) -> std::result::Result<f64, IbkrError>;
}

#[async_trait]
impl EquityFetcher for IbkrClient {
    async fn fetch_nlv(&self, account: &str) -> std::result::Result<f64, IbkrError> {
        let summary = self.get_account_summary(account).await?;
        let row = summary.iter().find(|s| s.tag == NLV_TAG).ok_or_else(|| {
            IbkrError::RequestFailed(format!(
                "{NLV_TAG} not present in account_summary for {account}"
            ))
        })?;
        row.value
            .parse::<f64>()
            .map_err(|e| IbkrError::SerializationError(format!("parse NLV '{}': {e}", row.value)))
    }
}

#[derive(Error, Debug)]
pub enum EquitySnapshotError {
    #[error("ibkr: {0}")]
    Ibkr(#[from] IbkrError),
    #[error("storage: {0}")]
    Storage(#[from] StorageError),
    #[error("no snapshot available for account '{0}'")]
    Missing(String),
}

pub type Result<T> = std::result::Result<T, EquitySnapshotError>;

#[derive(Clone)]
pub struct EquitySnapshotService {
    db: Arc<Db>,
    fetcher: Arc<dyn EquityFetcher>,
}

impl EquitySnapshotService {
    pub fn new(db: Arc<Db>, fetcher: Arc<dyn EquityFetcher>) -> Self {
        Self { db, fetcher }
    }

    /// Today's snapshot, fetching from IBKR if the cache is empty for
    /// the current ET trading date. On a fetch failure, falls back to
    /// the most-recent persisted row tagged `StaleCache`. If neither
    /// exists, returns `Missing` — the caller should propagate this
    /// as a non-sizing skip rather than guess at equity.
    pub async fn current(&self, account: &str) -> Result<EquitySnapshot> {
        let today = today_et();
        if let Some(snap) = self.read(account, &today).await? {
            return Ok(snap);
        }
        match self.fetcher.fetch_nlv(account).await {
            Ok(nlv) => {
                let snap = self
                    .persist(account, &today, nlv, EquitySource::IbkrAccountSummary)
                    .await?;
                Ok(snap)
            }
            Err(e) => {
                warn!(
                    "equity_snapshot: live fetch failed for {account} ({e}); falling back to stale cache"
                );
                self.read_latest(account)
                    .await?
                    .map(|mut snap| {
                        snap.source = EquitySource::StaleCache;
                        snap
                    })
                    .ok_or_else(|| EquitySnapshotError::Missing(account.to_string()))
            }
        }
    }

    /// Force a fresh IBKR fetch; overwrites today's row. Used after
    /// the trader does a deposit / withdrawal and wants sizing to
    /// reflect the new NLV without waiting for the next market open.
    pub async fn force_refresh(&self, account: &str) -> Result<EquitySnapshot> {
        let nlv = self.fetcher.fetch_nlv(account).await?;
        let today = today_et();
        self.persist(account, &today, nlv, EquitySource::IbkrAccountSummary)
            .await
    }

    /// Read the row for `account` on `as_of_date` if present.
    pub async fn read(
        &self,
        account: &str,
        as_of_date: &str,
    ) -> Result<Option<EquitySnapshot>> {
        let account = account.to_string();
        let date = as_of_date.to_string();
        let row = self
            .db
            .with_conn(move |conn| {
                conn.query_row(
                    "SELECT account, as_of_date, nlv_cents, source, fetched_at \
                     FROM equity_snapshots \
                     WHERE account = ?1 AND as_of_date = ?2",
                    rusqlite::params![account, date],
                    snapshot_row,
                )
                .optional()
                .map_err(StorageError::from)
            })
            .await?;
        match row {
            Some(r) => Ok(Some(decode_row(r)?)),
            None => Ok(None),
        }
    }

    async fn read_latest(&self, account: &str) -> Result<Option<EquitySnapshot>> {
        let account = account.to_string();
        let row = self
            .db
            .with_conn(move |conn| {
                conn.query_row(
                    "SELECT account, as_of_date, nlv_cents, source, fetched_at \
                     FROM equity_snapshots \
                     WHERE account = ?1 \
                     ORDER BY fetched_at DESC LIMIT 1",
                    rusqlite::params![account],
                    snapshot_row,
                )
                .optional()
                .map_err(StorageError::from)
            })
            .await?;
        match row {
            Some(r) => Ok(Some(decode_row(r)?)),
            None => Ok(None),
        }
    }

    async fn persist(
        &self,
        account: &str,
        as_of_date: &str,
        nlv: f64,
        source: EquitySource,
    ) -> Result<EquitySnapshot> {
        let nlv_cents = (nlv * 100.0).round() as i64;
        let fetched_at = Utc::now();
        let fetched_at_unix = fetched_at.timestamp();
        let account_owned = account.to_string();
        let date_owned = as_of_date.to_string();
        let source_str = source.as_str().to_string();

        let acc_for_db = account_owned.clone();
        let date_for_db = date_owned.clone();
        self.db
            .with_conn(move |conn| {
                conn.execute(
                    "INSERT INTO equity_snapshots \
                       (account, as_of_date, nlv_cents, source, fetched_at) \
                     VALUES (?1, ?2, ?3, ?4, ?5) \
                     ON CONFLICT(account, as_of_date) DO UPDATE SET \
                       nlv_cents = excluded.nlv_cents, \
                       source = excluded.source, \
                       fetched_at = excluded.fetched_at",
                    rusqlite::params![
                        acc_for_db,
                        date_for_db,
                        nlv_cents,
                        source_str,
                        fetched_at_unix,
                    ],
                )
                .map_err(StorageError::from)?;
                Ok(())
            })
            .await?;

        Ok(EquitySnapshot {
            account: account_owned,
            as_of_date: date_owned,
            nlv_cents,
            source,
            fetched_at,
        })
    }
}

fn today_et() -> String {
    Utc::now()
        .with_timezone(&New_York)
        .date_naive()
        .format("%Y-%m-%d")
        .to_string()
}

type SnapshotRow = (String, String, i64, String, i64);

fn snapshot_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SnapshotRow> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
    ))
}

fn decode_row(r: SnapshotRow) -> Result<EquitySnapshot> {
    let (account, as_of_date, nlv_cents, source_s, fetched_at_unix) = r;
    let source = EquitySource::parse(&source_s).ok_or_else(|| {
        EquitySnapshotError::Storage(StorageError::Migration(format!(
            "unknown equity_snapshots.source '{source_s}' for {account}"
        )))
    })?;
    // Reject ill-formed dates so downstream date arithmetic is safe.
    NaiveDate::parse_from_str(&as_of_date, "%Y-%m-%d").map_err(|e| {
        EquitySnapshotError::Storage(StorageError::Migration(format!(
            "bad as_of_date '{as_of_date}' for {account}: {e}"
        )))
    })?;
    let fetched_at: DateTime<Utc> = DateTime::from_timestamp(fetched_at_unix, 0).ok_or_else(|| {
        EquitySnapshotError::Storage(StorageError::Migration(format!(
            "bad fetched_at unix '{fetched_at_unix}' for {account}"
        )))
    })?;
    Ok(EquitySnapshot {
        account,
        as_of_date,
        nlv_cents,
        source,
        fetched_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use tempfile::NamedTempFile;

    struct CountingFetcher {
        nlv: Mutex<f64>,
        calls: Mutex<usize>,
        fail_next: Mutex<bool>,
    }

    impl CountingFetcher {
        fn new(nlv: f64) -> Arc<Self> {
            Arc::new(Self {
                nlv: Mutex::new(nlv),
                calls: Mutex::new(0),
                fail_next: Mutex::new(false),
            })
        }
    }

    #[async_trait]
    impl EquityFetcher for CountingFetcher {
        async fn fetch_nlv(&self, _account: &str) -> std::result::Result<f64, IbkrError> {
            *self.calls.lock().unwrap() += 1;
            if *self.fail_next.lock().unwrap() {
                *self.fail_next.lock().unwrap() = false;
                return Err(IbkrError::NotConnected);
            }
            Ok(*self.nlv.lock().unwrap())
        }
    }

    fn make_db() -> (NamedTempFile, Arc<Db>) {
        let tmp = NamedTempFile::new().unwrap();
        let db = Arc::new(Db::open(tmp.path()).unwrap());
        (tmp, db)
    }

    #[tokio::test]
    async fn first_call_fetches_then_caches() {
        let (_tmp, db) = make_db();
        let fetcher = CountingFetcher::new(123_456.78);
        let svc = EquitySnapshotService::new(db, fetcher.clone());

        let snap1 = svc.current("DU1").await.unwrap();
        assert_eq!(snap1.nlv_cents, 12_345_678);
        assert_eq!(snap1.source, EquitySource::IbkrAccountSummary);

        let snap2 = svc.current("DU1").await.unwrap();
        assert_eq!(snap2.nlv_cents, snap1.nlv_cents);
        assert_eq!(snap2.as_of_date, snap1.as_of_date);
        assert_eq!(*fetcher.calls.lock().unwrap(), 1, "second read served from cache");
    }

    #[tokio::test]
    async fn force_refresh_overwrites_today() {
        let (_tmp, db) = make_db();
        let fetcher = CountingFetcher::new(100_000.0);
        let svc = EquitySnapshotService::new(db, fetcher.clone());

        svc.current("DU1").await.unwrap();
        *fetcher.nlv.lock().unwrap() = 200_000.0;
        let refreshed = svc.force_refresh("DU1").await.unwrap();
        assert_eq!(refreshed.nlv_cents, 20_000_000);

        let read_back = svc.current("DU1").await.unwrap();
        assert_eq!(read_back.nlv_cents, 20_000_000);
    }

    #[tokio::test]
    async fn fetch_failure_falls_back_to_stale_cache() {
        let (_tmp, db) = make_db();
        let fetcher = CountingFetcher::new(100_000.0);
        let svc = EquitySnapshotService::new(Arc::clone(&db), fetcher.clone());

        // Seed a yesterday row by writing directly so today's `current`
        // can't read-through (no row for today).
        let yesterday = "2026-05-04";
        svc.persist("DU1", yesterday, 99_000.0, EquitySource::IbkrAccountSummary)
            .await
            .unwrap();

        *fetcher.fail_next.lock().unwrap() = true;
        let snap = svc.current("DU1").await.unwrap();
        assert_eq!(snap.source, EquitySource::StaleCache);
        assert_eq!(snap.nlv_cents, 9_900_000);
    }

    #[tokio::test]
    async fn missing_when_no_row_and_fetch_fails() {
        let (_tmp, db) = make_db();
        let fetcher = CountingFetcher::new(0.0);
        *fetcher.fail_next.lock().unwrap() = true;
        let svc = EquitySnapshotService::new(db, fetcher);

        let err = svc.current("DU_UNSEEN").await.unwrap_err();
        assert!(matches!(err, EquitySnapshotError::Missing(_)));
    }

    #[tokio::test]
    async fn mock_ibkr_client_pulls_nlv_from_account_summary() {
        use crate::ibkr::mocks::{test_fixtures, IbkrClientTrait, MockIbkrClient};

        struct MockFetcher(Arc<MockIbkrClient>);
        #[async_trait]
        impl EquityFetcher for MockFetcher {
            async fn fetch_nlv(&self, account: &str) -> std::result::Result<f64, IbkrError> {
                let summary = self.0.get_account_summary(account).await?;
                let row = summary
                    .iter()
                    .find(|s| s.tag == NLV_TAG)
                    .ok_or_else(|| IbkrError::RequestFailed("no nlv".into()))?;
                row.value
                    .parse::<f64>()
                    .map_err(|e| IbkrError::SerializationError(e.to_string()))
            }
        }

        let mock = Arc::new(MockIbkrClient::new());
        mock.set_connected(true).await;
        mock.set_account_summary(test_fixtures::sample_account_summary())
            .await;
        let fetcher = MockFetcher(Arc::clone(&mock));
        let nlv = fetcher.fetch_nlv("DU123456").await.unwrap();
        // Fixture pins NetLiquidation = 100000.0.
        assert!((nlv - 100_000.0).abs() < 1e-9);
    }
}
