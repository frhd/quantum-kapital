//! Narrow IBKR seam for the MCP live-account tools (`get_positions`,
//! `get_account_summary`).
//!
//! Mirrors the `MarketScanner` / `QuoteFetcher` / `HistoricalDataFetcher`
//! pattern used elsewhere in the codebase: a tiny trait covering only
//! the methods the tools actually call, with a production impl on
//! `IbkrClient` and a `#[cfg(test)]` impl on `MockIbkrClient` so the
//! per-tool unit tests never reach a live TWS.
//!
//! Why a narrow trait instead of the test-only `IbkrClientTrait` from
//! `ibkr/mocks.rs`: that trait is intentionally `#[cfg(test)]`-gated and
//! enumerates every method the legacy command tests exercised. Lifting
//! it into production would force a production stub on `IbkrClient` for
//! a dozen methods the rest of the app already calls inherently —
//! noise for a tool surface that needs only two methods.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{NaiveDate, Utc};
use chrono_tz::America::New_York;

use crate::ibkr::error::{IbkrError, Result as IbkrResult};
use crate::ibkr::types::{AccountSummary, IbkrExecution, Position};
use crate::mcp::tools::executions::ExecutionRow;
use crate::services::executions::ExecutionsStore;

/// Narrow seam for the MCP account tools. Returns an error if the
/// underlying IBKR connection isn't live; the tool layer surfaces that
/// to the agent unchanged.
///
/// `list_accounts` exists so the tool layer can resolve an optional
/// `account` arg — defaulting it when there's exactly one managed
/// account, and erroring (with the list of choices) when there are
/// multiple. Without this method on the seam the tools couldn't know
/// "is the user's choice valid?" without bypassing the trait into the
/// concrete `IbkrClient`.
///
/// `executions` is the same seam used by `get_executions`. The IBKR
/// adapter's `executions(date)` returns rows for **all** managed
/// accounts in a single drain (the API does not support a server-side
/// per-account filter), so the production impl filters by `account`
/// after fetching and converts each row to the wire-stable
/// [`ExecutionRow`].
#[async_trait]
pub trait AccountReader: Send + Sync {
    async fn list_accounts(&self) -> IbkrResult<Vec<String>>;
    async fn get_positions(&self, account: &str) -> IbkrResult<Vec<Position>>;
    async fn get_account_summary(&self, account: &str) -> IbkrResult<Vec<AccountSummary>>;
    async fn executions(&self, account: &str, date: NaiveDate) -> IbkrResult<Vec<ExecutionRow>>;
}

/// Inner seam consumed by [`ProdAccountReader`]. Combines the four
/// live-IBKR methods the wrapper needs into a single trait so the
/// wrapper itself can be unit-tested with `MockIbkrClient`.
#[async_trait]
pub trait LiveAccountClient: Send + Sync {
    async fn list_accounts(&self) -> IbkrResult<Vec<String>>;
    async fn get_positions(&self, account: &str) -> IbkrResult<Vec<Position>>;
    async fn get_account_summary(&self, account: &str) -> IbkrResult<Vec<AccountSummary>>;
    async fn fetch_executions(&self, date: NaiveDate) -> IbkrResult<Vec<IbkrExecution>>;
}

#[async_trait]
impl LiveAccountClient for crate::ibkr::client::IbkrClient {
    async fn list_accounts(&self) -> IbkrResult<Vec<String>> {
        crate::ibkr::client::IbkrClient::get_accounts(self).await
    }

    async fn get_positions(&self, account: &str) -> IbkrResult<Vec<Position>> {
        crate::ibkr::client::IbkrClient::get_positions(self, account).await
    }

    async fn get_account_summary(&self, account: &str) -> IbkrResult<Vec<AccountSummary>> {
        crate::ibkr::client::IbkrClient::get_account_summary(self, account).await
    }

    async fn fetch_executions(&self, date: NaiveDate) -> IbkrResult<Vec<IbkrExecution>> {
        crate::ibkr::client::IbkrClient::executions(self, date).await
    }
}

#[cfg(test)]
#[async_trait]
impl LiveAccountClient for crate::ibkr::mocks::MockIbkrClient {
    async fn list_accounts(&self) -> IbkrResult<Vec<String>> {
        crate::ibkr::mocks::IbkrClientTrait::get_accounts(self).await
    }

    async fn get_positions(&self, account: &str) -> IbkrResult<Vec<Position>> {
        crate::ibkr::mocks::IbkrClientTrait::get_positions(self, account).await
    }

    async fn get_account_summary(&self, account: &str) -> IbkrResult<Vec<AccountSummary>> {
        crate::ibkr::mocks::IbkrClientTrait::get_account_summary(self, account).await
    }

    async fn fetch_executions(&self, date: NaiveDate) -> IbkrResult<Vec<IbkrExecution>> {
        crate::ibkr::mocks::MockIbkrClient::executions(self, date).await
    }
}

/// Production [`AccountReader`] for the live MCP server.
///
/// Wraps the IBKR client and the persistent `ExecutionsStore` so that
/// `executions(account, date)`:
/// - past day (`date < today_et`): served from the store. If the store
///   has no rows for that day (e.g. the app wasn't running when the
///   trades happened), falls back to a live IBKR drain via the
///   `specific_dates` filter — IBKR retains executions for ~7 trading
///   days — and records what comes back so the next read is local.
/// - today (`date == today_et`): drained live from IBKR; the full
///   batch (all managed accounts) is recorded into the store, then
///   the store is queried so the response is consistent with what a
///   later "yesterday" call will see.
/// - future (`date > today_et`): empty.
///
/// `list_accounts`, `get_positions`, `get_account_summary` forward to
/// the IBKR client unchanged.
pub struct ProdAccountReader {
    client: Arc<dyn LiveAccountClient>,
    store: Arc<ExecutionsStore>,
}

impl ProdAccountReader {
    pub fn new(client: Arc<dyn LiveAccountClient>, store: Arc<ExecutionsStore>) -> Self {
        Self { client, store }
    }
}

#[async_trait]
impl AccountReader for ProdAccountReader {
    async fn list_accounts(&self) -> IbkrResult<Vec<String>> {
        self.client.list_accounts().await
    }

    async fn get_positions(&self, account: &str) -> IbkrResult<Vec<Position>> {
        self.client.get_positions(account).await
    }

    async fn get_account_summary(&self, account: &str) -> IbkrResult<Vec<AccountSummary>> {
        self.client.get_account_summary(account).await
    }

    async fn executions(&self, account: &str, date: NaiveDate) -> IbkrResult<Vec<ExecutionRow>> {
        let today_et = Utc::now().with_timezone(&New_York).date_naive();
        if date > today_et {
            return Ok(Vec::new());
        }
        if date < today_et {
            let rows = self
                .store
                .query(account, date, None)
                .await
                .map_err(|e| IbkrError::Unknown(format!("executions store: {e}")))?;
            if !rows.is_empty() {
                return Ok(rows.into_iter().map(ExecutionRow::from_ibkr).collect());
            }
            // Store has nothing for this day. Try a live drain — IBKR's
            // `specific_dates` filter retains the last ~7 trading days,
            // so a back-fill is usually possible. On rows, record into
            // the store and re-query so the result mirrors today's
            // shape; on empty/error, mirror today's branch (empty/Err).
            return match self.client.fetch_executions(date).await {
                Ok(live) if live.is_empty() => Ok(Vec::new()),
                Ok(live) => {
                    if let Err(e) = self.store.record(&live).await {
                        tracing::warn!(error = %e, "executions store record failed; serving live");
                        return Ok(live
                            .into_iter()
                            .filter(|r| r.account == account)
                            .map(ExecutionRow::from_ibkr)
                            .collect());
                    }
                    let rows = self
                        .store
                        .query(account, date, None)
                        .await
                        .map_err(|e| IbkrError::Unknown(format!("executions store: {e}")))?;
                    Ok(rows.into_iter().map(ExecutionRow::from_ibkr).collect())
                }
                Err(e) => Err(e),
            };
        }
        // Today: drain live IBKR for **all** managed accounts (IBKR's
        // server-side filter is date-only), record the full batch into
        // the store, then query back filtered to this account so a
        // subsequent past-day call sees the same rows.
        match self.client.fetch_executions(date).await {
            Ok(live) if live.is_empty() => Ok(Vec::new()),
            Ok(live) => {
                if let Err(e) = self.store.record(&live).await {
                    tracing::warn!(error = %e, "executions store record failed; serving live");
                    return Ok(live
                        .into_iter()
                        .filter(|r| r.account == account)
                        .map(ExecutionRow::from_ibkr)
                        .collect());
                }
                let rows = self
                    .store
                    .query(account, date, None)
                    .await
                    .map_err(|e| IbkrError::Unknown(format!("executions store: {e}")))?;
                Ok(rows.into_iter().map(ExecutionRow::from_ibkr).collect())
            }
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
#[async_trait]
impl AccountReader for crate::ibkr::mocks::MockIbkrClient {
    async fn list_accounts(&self) -> IbkrResult<Vec<String>> {
        crate::ibkr::mocks::IbkrClientTrait::get_accounts(self).await
    }

    async fn get_positions(&self, account: &str) -> IbkrResult<Vec<Position>> {
        crate::ibkr::mocks::IbkrClientTrait::get_positions(self, account).await
    }

    async fn get_account_summary(&self, account: &str) -> IbkrResult<Vec<AccountSummary>> {
        crate::ibkr::mocks::IbkrClientTrait::get_account_summary(self, account).await
    }

    async fn executions(&self, account: &str, date: NaiveDate) -> IbkrResult<Vec<ExecutionRow>> {
        // Disambiguate from this trait method (same name, 2 args vs 1):
        // the inherent `executions(date)` on `MockIbkrClient` already
        // filters by ET trading day, so we just need to drop foreign
        // accounts and project to the wire DTO.
        let rows = crate::ibkr::mocks::MockIbkrClient::executions(self, date).await?;
        Ok(rows
            .into_iter()
            .filter(|r| r.account == account)
            .map(ExecutionRow::from_ibkr)
            .collect())
    }
}
