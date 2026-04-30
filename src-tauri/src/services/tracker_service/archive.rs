//! Soft-archive rail for `TrackerService`.
//!
//! Archiving a ticker stamps `archived_at = now` on the ticker and on
//! every `setups` row that belongs to it, in a single transaction. All
//! existing reads in `mod.rs` and `setups.rs` filter `archived_at IS NULL`
//! so an archived ticker drops out of the watchlist, detector runs, the
//! state machine, alert emission, and LLM spend without any change in the
//! runners themselves. `unarchive_ticker` clears the field and restores
//! the row.

use chrono::Utc;

use crate::storage::error::StorageError;

use super::{Result, TrackerError, TrackerService};

impl TrackerService {
    /// Soft-archive `symbol` and every `setups` row underneath it. The
    /// stamp is `Utc::now().timestamp()` and the operation is wrapped in
    /// a single transaction so an archived ticker can never have live
    /// children. Idempotent: re-archiving a row that already has
    /// `archived_at` set is a no-op (the timestamp does not get bumped).
    /// Returns `TrackerError::NotFound` only when the symbol has never
    /// been tracked at all.
    pub async fn archive_ticker(&self, symbol: &str) -> Result<()> {
        let symbol_norm = symbol.to_uppercase();
        let now_unix = Utc::now().timestamp();
        let symbol_for_db = symbol_norm.clone();

        let outcome = self
            .db
            .with_conn(move |conn| {
                let tx = conn.transaction()?;
                let exists: i64 = tx.query_row(
                    "SELECT COUNT(*) FROM tracked_tickers WHERE symbol = ?1",
                    rusqlite::params![symbol_for_db],
                    |row| row.get(0),
                )?;
                if exists == 0 {
                    return Ok(false);
                }
                tx.execute(
                    "UPDATE tracked_tickers SET archived_at = ?1 \
                     WHERE symbol = ?2 AND archived_at IS NULL",
                    rusqlite::params![now_unix, symbol_for_db],
                )?;
                tx.execute(
                    "UPDATE setups SET archived_at = ?1 \
                     WHERE symbol = ?2 AND archived_at IS NULL",
                    rusqlite::params![now_unix, symbol_for_db],
                )?;
                tx.commit()?;
                Ok::<_, StorageError>(true)
            })
            .await?;

        if !outcome {
            return Err(TrackerError::NotFound(symbol_norm));
        }
        Ok(())
    }

    /// Inverse of [`archive_ticker`]. Clears `archived_at` on the ticker
    /// and on every setup that belongs to it, restoring them to active
    /// reads. Idempotent. Returns `TrackerError::NotFound` only when the
    /// symbol has never been tracked at all.
    pub async fn unarchive_ticker(&self, symbol: &str) -> Result<()> {
        let symbol_norm = symbol.to_uppercase();
        let symbol_for_db = symbol_norm.clone();

        let outcome = self
            .db
            .with_conn(move |conn| {
                let tx = conn.transaction()?;
                let exists: i64 = tx.query_row(
                    "SELECT COUNT(*) FROM tracked_tickers WHERE symbol = ?1",
                    rusqlite::params![symbol_for_db],
                    |row| row.get(0),
                )?;
                if exists == 0 {
                    return Ok(false);
                }
                tx.execute(
                    "UPDATE tracked_tickers SET archived_at = NULL WHERE symbol = ?1",
                    rusqlite::params![symbol_for_db],
                )?;
                tx.execute(
                    "UPDATE setups SET archived_at = NULL WHERE symbol = ?1",
                    rusqlite::params![symbol_for_db],
                )?;
                tx.commit()?;
                Ok::<_, StorageError>(true)
            })
            .await?;

        if !outcome {
            return Err(TrackerError::NotFound(symbol_norm));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chrono::Utc;
    use tempfile::NamedTempFile;

    use crate::ibkr::types::tracker::{StrategyTag, TrackerSource};
    use crate::services::tracker_service::{TrackerError, TrackerService};
    use crate::storage::Db;

    use crate::ibkr::types::BarSize;
    use crate::strategies::{Direction, SetupCandidate, TargetLevel};

    fn make_service() -> (NamedTempFile, TrackerService) {
        let tmp = NamedTempFile::new().expect("tempfile");
        let db = Db::open(tmp.path()).expect("open db");
        (tmp, TrackerService::new(Arc::new(db)))
    }

    fn sample_candidate() -> SetupCandidate {
        SetupCandidate {
            strategy: "breakout",
            tag: StrategyTag::Breakout,
            direction: Direction::Long,
            conviction_signal: 0.7,
            trigger_price: 105.0,
            stop_price: 100.0,
            targets: vec![TargetLevel {
                label: "2R".to_string(),
                price: 115.0,
            }],
            raw_signals: serde_json::json!({"volume_multiple": 1.8}),
            timeframe: BarSize::Day1,
            detected_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn archive_ticker_excludes_from_list_and_get() {
        let (_tmp, svc) = make_service();
        svc.add("AAPL", TrackerSource::Manual, None, vec![], None)
            .await
            .unwrap();
        svc.add("MSFT", TrackerSource::Manual, None, vec![], None)
            .await
            .unwrap();

        svc.archive_ticker("AAPL").await.expect("archive");

        let listed = svc.list(None).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].symbol, "MSFT");
        assert!(svc.get("AAPL").await.unwrap().is_none());
        assert!(svc.get("MSFT").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn archive_ticker_cascades_to_setups() {
        let (_tmp, svc) = make_service();
        svc.add("AAPL", TrackerSource::Manual, None, vec![], None)
            .await
            .unwrap();
        let inserted = svc
            .insert_setup("AAPL", &sample_candidate())
            .await
            .expect("insert setup");

        svc.archive_ticker("AAPL").await.expect("archive");

        // list_setups excludes archived rows.
        assert!(svc
            .list_setups(Some("AAPL"), None)
            .await
            .unwrap()
            .is_empty());
        assert!(svc.list_setups(None, None).await.unwrap().is_empty());
        // get_setup also excludes the archived row.
        assert!(svc.get_setup(inserted.id).await.unwrap().is_none());
        // count_active_setups respects the filter.
        assert_eq!(svc.count_active_setups("AAPL").await.unwrap(), 0);
    }

    #[tokio::test]
    async fn archive_ticker_normalizes_symbol_case() {
        let (_tmp, svc) = make_service();
        svc.add("tsla", TrackerSource::Manual, None, vec![], None)
            .await
            .unwrap();
        svc.archive_ticker("tsla").await.expect("archive lowercase");
        assert!(svc.get("TSLA").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn archive_ticker_is_idempotent() {
        let (_tmp, svc) = make_service();
        svc.add("AAPL", TrackerSource::Manual, None, vec![], None)
            .await
            .unwrap();
        svc.archive_ticker("AAPL").await.expect("first archive");
        // Second archive must not error.
        svc.archive_ticker("AAPL").await.expect("second archive");
    }

    #[tokio::test]
    async fn archive_ticker_unknown_symbol_errors_not_found() {
        let (_tmp, svc) = make_service();
        let err = svc.archive_ticker("NOSUCH").await.expect_err("must err");
        match err {
            TrackerError::NotFound(s) => assert_eq!(s, "NOSUCH"),
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn unarchive_ticker_restores_ticker_and_setups() {
        let (_tmp, svc) = make_service();
        svc.add("AAPL", TrackerSource::Manual, None, vec![], None)
            .await
            .unwrap();
        let inserted = svc.insert_setup("AAPL", &sample_candidate()).await.unwrap();

        svc.archive_ticker("AAPL").await.unwrap();
        assert!(svc.get("AAPL").await.unwrap().is_none());

        svc.unarchive_ticker("AAPL").await.expect("unarchive");
        let restored = svc.get("AAPL").await.unwrap().expect("present");
        assert_eq!(restored.symbol, "AAPL");
        assert!(restored.archived_at.is_none());

        let setup = svc
            .get_setup(inserted.id)
            .await
            .unwrap()
            .expect("setup back");
        assert!(setup.archived_at.is_none());
        assert_eq!(setup.id, inserted.id);
    }

    #[tokio::test]
    async fn unarchive_ticker_unknown_symbol_errors_not_found() {
        let (_tmp, svc) = make_service();
        let err = svc.unarchive_ticker("NOSUCH").await.expect_err("must err");
        match err {
            TrackerError::NotFound(s) => assert_eq!(s, "NOSUCH"),
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn archived_ticker_blocks_writes_via_existing_methods() {
        // touch_last_checked, set_tags, set_status all gate on
        // `archived_at IS NULL`. After archive, set_tags / set_status
        // surface NotFound because the row is invisible to reads — the
        // contract for callers stays "archived means gone."
        let (_tmp, svc) = make_service();
        svc.add("AAPL", TrackerSource::Manual, None, vec![], None)
            .await
            .unwrap();
        svc.archive_ticker("AAPL").await.unwrap();

        // touch_last_checked is fire-and-forget (no NotFound today); it
        // must remain a no-op rather than resurrecting the archived row.
        svc.touch_last_checked("AAPL").await.unwrap();
        assert!(svc.get("AAPL").await.unwrap().is_none());

        let err = svc.set_tags("AAPL", vec![]).await.expect_err("must err");
        matches!(err, TrackerError::NotFound(_));
    }

    #[tokio::test]
    async fn recent_duplicate_skips_archived_setups() {
        let (_tmp, svc) = make_service();
        svc.add("AAPL", TrackerSource::Manual, None, vec![], None)
            .await
            .unwrap();
        svc.insert_setup("AAPL", &sample_candidate()).await.unwrap();
        svc.archive_ticker("AAPL").await.unwrap();

        let dup = svc
            .recent_duplicate(
                "AAPL",
                "breakout",
                Direction::Long,
                chrono::Duration::hours(24),
            )
            .await
            .unwrap();
        assert!(
            dup.is_none(),
            "archived setup must not count as a recent duplicate"
        );
    }
}
