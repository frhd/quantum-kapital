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

use async_trait::async_trait;
use chrono::NaiveDate;

use crate::ibkr::error::Result as IbkrResult;
use crate::ibkr::types::{AccountSummary, Position};
use crate::mcp::tools::executions::ExecutionRow;

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

#[async_trait]
impl AccountReader for crate::ibkr::client::IbkrClient {
    async fn list_accounts(&self) -> IbkrResult<Vec<String>> {
        crate::ibkr::client::IbkrClient::get_accounts(self).await
    }

    async fn get_positions(&self, account: &str) -> IbkrResult<Vec<Position>> {
        crate::ibkr::client::IbkrClient::get_positions(self, account).await
    }

    async fn get_account_summary(&self, account: &str) -> IbkrResult<Vec<AccountSummary>> {
        crate::ibkr::client::IbkrClient::get_account_summary(self, account).await
    }

    async fn executions(&self, account: &str, date: NaiveDate) -> IbkrResult<Vec<ExecutionRow>> {
        let rows = crate::ibkr::client::IbkrClient::executions(self, date).await?;
        Ok(rows
            .into_iter()
            .filter(|r| r.account == account)
            .map(ExecutionRow::from_ibkr)
            .collect())
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
