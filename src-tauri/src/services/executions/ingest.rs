//! `ExecutionsIngestor` — background task that drains live IBKR fills
//! into the `executions` store every 5 min during market hours.
//!
//! Runs alongside the `ProdAccountReader::executions(today)`
//! opportunistic refresh; together they make the store
//! eventually-consistent with IBKR within the 5-min poll window.
//!
//! The seam trait `LiveExecutionsFetcher` is the production analogue
//! of the test-only `IbkrClientTrait` from `ibkr/mocks.rs` — a
//! deliberately tiny surface (one fetch method) so the ingestor
//! depends only on what it actually needs and the wiring in `lib.rs`
//! can pass `Arc<IbkrClient>` directly.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{NaiveDate, Timelike, Utc};
use chrono_tz::America::New_York;
use tracing::{info, warn};

use crate::ibkr::client::{IbkrClient, StreamHandle};
use crate::ibkr::error::Result as IbkrResult;
use crate::ibkr::types::IbkrExecution;
use crate::services::tca::TcaService;

use super::store::ExecutionsStore;

const POLL_INTERVAL: Duration = Duration::from_secs(5 * 60);
const MARKET_OPEN_HOUR_ET: u32 = 4; // pre-market starts 04:00 ET
const MARKET_CLOSE_HOUR_ET: u32 = 20; // after-hours ends 20:00 ET

/// Tiny seam over `IbkrClient::executions(date)`. Production wiring
/// uses the inherent impl on `IbkrClient`; tests substitute a mock.
#[async_trait]
pub trait LiveExecutionsFetcher: Send + Sync {
    /// Drain IBKR fills for the requested ET trading date for **all**
    /// managed accounts (IBKR's `reqExecutions` does not allow a
    /// server-side per-account filter). The ingestor records the full
    /// batch into the store and the store's `account` column carries
    /// the per-row attribution.
    async fn fetch_executions(&self, date: NaiveDate) -> IbkrResult<Vec<IbkrExecution>>;
}

#[async_trait]
impl LiveExecutionsFetcher for IbkrClient {
    async fn fetch_executions(&self, date: NaiveDate) -> IbkrResult<Vec<IbkrExecution>> {
        IbkrClient::executions(self, date).await
    }
}

#[cfg(test)]
#[async_trait]
impl LiveExecutionsFetcher for crate::ibkr::mocks::MockIbkrClient {
    async fn fetch_executions(&self, date: NaiveDate) -> IbkrResult<Vec<IbkrExecution>> {
        crate::ibkr::mocks::MockIbkrClient::executions(self, date).await
    }
}

#[derive(Clone)]
pub struct ExecutionsIngestor {
    store: Arc<ExecutionsStore>,
    fetcher: Arc<dyn LiveExecutionsFetcher>,
    /// Phase 2 — set after `store.record` succeeds, the ingestor
    /// asks TCA to stamp `setup_id` / `intent_id` / slippage on the
    /// freshly-stored rows. Optional so the executions service can
    /// still be wired without TCA in tests + early bring-up.
    tca: Option<Arc<TcaService>>,
}

impl ExecutionsIngestor {
    pub fn new(store: Arc<ExecutionsStore>, fetcher: Arc<dyn LiveExecutionsFetcher>) -> Self {
        Self {
            store,
            fetcher,
            tca: None,
        }
    }

    /// Attach a TCA service. The ingestor calls
    /// `expire_stale` + per-account `attach_fills_for_account_today`
    /// after every successful `store.record` batch.
    pub fn with_tca(mut self, tca: Arc<TcaService>) -> Self {
        self.tca = Some(tca);
        self
    }

    /// One drain pass. Public for tests; otherwise called from `spawn`.
    pub async fn tick_once(&self) {
        if !in_market_hours_et() {
            tracing::debug!("executions ingestor idle (outside market hours)");
            return;
        }
        let today_et = Utc::now().with_timezone(&New_York).date_naive();
        match self.fetcher.fetch_executions(today_et).await {
            Ok(rows) if rows.is_empty() => {}
            Ok(rows) => {
                if let Err(e) = self.store.record(&rows).await {
                    warn!(error = %e, "executions ingestor: store.record failed");
                    return;
                }
                if let Some(tca) = self.tca.as_ref() {
                    if let Err(e) = tca.expire_stale().await {
                        warn!(error = %e, "tca: expire_stale failed");
                    }
                    let accounts: BTreeSet<String> =
                        rows.iter().map(|r| r.account.clone()).collect();
                    for account in accounts {
                        if let Err(e) = tca.attach_fills_for_account_today(&account).await {
                            warn!(account = %account, error = %e, "tca: attach failed");
                        }
                    }
                }
            }
            Err(e) => {
                tracing::debug!(error = %e, "executions ingestor: drain failed");
            }
        }
    }

    /// Spawn the long-lived loop. Returns a [`StreamHandle`] the caller
    /// holds + stops on shutdown — same shape as the EOD / intraday /
    /// social-sentiment schedulers. The first tick fires immediately so
    /// a freshly-started app primes the store without waiting 5 min.
    pub fn spawn(self: Arc<Self>) -> StreamHandle {
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_task = Arc::clone(&shutdown);
        let ingestor = Arc::clone(&self);

        let join = tokio::spawn(async move {
            ingestor.tick_once().await;
            let mut ticker = tokio::time::interval(POLL_INTERVAL);
            ticker.tick().await; // discard the immediate first tick
            loop {
                if shutdown_task.load(Ordering::Relaxed) {
                    break;
                }
                ticker.tick().await;
                if shutdown_task.load(Ordering::Relaxed) {
                    break;
                }
                ingestor.tick_once().await;
            }
            info!("executions ingestor stopped");
        });

        StreamHandle::new("executions ingestor", shutdown, join)
    }
}

fn in_market_hours_et() -> bool {
    let now_et = Utc::now().with_timezone(&New_York);
    let h = now_et.hour();
    (MARKET_OPEN_HOUR_ET..MARKET_CLOSE_HOUR_ET).contains(&h)
}
