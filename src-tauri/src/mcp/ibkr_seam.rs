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

use crate::ibkr::error::Result as IbkrResult;
use crate::ibkr::types::{AccountSummary, Position};

/// Two-method seam for the MCP account tools. Returns an error if the
/// underlying IBKR connection isn't live; the tool layer surfaces that
/// to the agent unchanged.
#[async_trait]
pub trait AccountReader: Send + Sync {
    async fn get_positions(&self, account: &str) -> IbkrResult<Vec<Position>>;
    async fn get_account_summary(&self, account: &str) -> IbkrResult<Vec<AccountSummary>>;
}

#[async_trait]
impl AccountReader for crate::ibkr::client::IbkrClient {
    /// Inherent `IbkrClient::get_positions` queries the connected TWS
    /// session's first account — TWS itself only ever reports the
    /// connected account's positions, so the `account` arg is
    /// informational. The mock honours it for test fixtures.
    async fn get_positions(&self, _account: &str) -> IbkrResult<Vec<Position>> {
        crate::ibkr::client::IbkrClient::get_positions(self).await
    }

    async fn get_account_summary(&self, account: &str) -> IbkrResult<Vec<AccountSummary>> {
        crate::ibkr::client::IbkrClient::get_account_summary(self, account).await
    }
}

#[cfg(test)]
#[async_trait]
impl AccountReader for crate::ibkr::mocks::MockIbkrClient {
    async fn get_positions(&self, account: &str) -> IbkrResult<Vec<Position>> {
        crate::ibkr::mocks::IbkrClientTrait::get_positions(self, account).await
    }

    async fn get_account_summary(&self, account: &str) -> IbkrResult<Vec<AccountSummary>> {
        crate::ibkr::mocks::IbkrClientTrait::get_account_summary(self, account).await
    }
}
