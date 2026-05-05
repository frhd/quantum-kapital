//! Phase 2 — `services/tca/`: setup ↔ execution linkage + per-strategy
//! attribution.
//!
//! `TcaService` is the public seam over three pieces:
//! - `OrderIntentStore` — DB I/O for `order_intents` + linkage UPDATE
//!   on `executions`.
//! - `matcher` (pure) — picks an open intent for a freshly-arrived
//!   fill and computes slippage.
//! - `AttributionService` — read-only rollup queries for the UI.
//!
//! Wired into `lib.rs::run` and called by:
//! - `ExecutionsIngestor::tick_once` — after a `store.record` batch
//!   succeeds, runs `attach_fills_for_account_today` so newly-stored
//!   rows pick up their setup_id / slippage in the same poll.
//! - `tca_record_manual_intent` Tauri command — trader-initiated
//!   intent for an order placed outside our UI.
//! - `ibkr_place_order` — extended in this phase to record an intent
//!   before sending to IBKR.

mod attribution;
mod intent;
mod matcher;
mod types;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use chrono::{NaiveDate, Utc};
use thiserror::Error;
use tracing::{debug, warn};

use crate::ibkr::types::IbkrExecution;
use crate::services::executions::ExecutionsStore;
use crate::storage::error::StorageError;
use crate::storage::Db;

pub use attribution::AttributionService;
pub use intent::{NewOrderIntent, OrderIntentStore};
pub use matcher::{execution_side_to_intent_side, match_fill};
pub use types::{
    AttributionRow, IntendedPriceSource, IntentSide, MatchWindow, SlippageDistributionRow,
};
// Re-exports kept available for downstream consumers (P3 brackets,
// MCP read tools) that need the full type set even when the lib's
// internal call sites currently use a subset.
#[allow(unused_imports)]
pub use matcher::compute_slippage;
#[allow(unused_imports)]
pub use types::{
    default_histogram_edges, IntentStatus, LinkageDecision, OrderIntent, SlippageBucket,
    SlippageRecord,
};

#[derive(Error, Debug)]
pub enum TcaError {
    #[error("storage: {0}")]
    Storage(#[from] StorageError),
    #[error("invalid argument: {0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, TcaError>;

/// The orchestrating service. Cheap to clone — internally it's two
/// Arcs (`Db`-backed stores).
#[derive(Clone)]
pub struct TcaService {
    intents: Arc<OrderIntentStore>,
    attribution: Arc<AttributionService>,
    executions: Arc<ExecutionsStore>,
    /// Reserved for P3 — bracket-attach uses a tighter window than
    /// the parent intent. Default value is the same MatchWindow the
    /// matcher reads internally.
    #[allow(dead_code)]
    window: MatchWindow,
}

impl TcaService {
    pub fn new(db: Arc<Db>, executions: Arc<ExecutionsStore>) -> Self {
        Self {
            intents: Arc::new(OrderIntentStore::new(Arc::clone(&db))),
            attribution: Arc::new(AttributionService::new(db)),
            executions,
            window: MatchWindow::default(),
        }
    }

    #[allow(dead_code)] // exercised by tests + reserved for P3 commands
    pub fn intents(&self) -> &OrderIntentStore {
        &self.intents
    }

    pub fn attribution(&self) -> &AttributionService {
        &self.attribution
    }

    /// Record a new intent. Validates that price > 0 and qty > 0.
    pub async fn record_intent(&self, intent: NewOrderIntent) -> Result<()> {
        if !intent.qty.is_finite() || intent.qty <= 0.0 {
            return Err(TcaError::Invalid("qty must be > 0".to_string()));
        }
        if intent.intended_price_cents <= 0 {
            return Err(TcaError::Invalid(
                "intended_price_cents must be > 0".to_string(),
            ));
        }
        if intent.expires_at <= intent.posted_at {
            return Err(TcaError::Invalid(
                "expires_at must be > posted_at".to_string(),
            ));
        }
        self.intents.insert(intent).await?;
        Ok(())
    }

    /// Try to attach an intent linkage to a single fill. Public for
    /// test ergonomics; production flow uses
    /// `attach_fills_for_account_today`.
    pub async fn attach_fill(&self, fill: &IbkrExecution) -> Result<Option<LinkageDecision>> {
        let side = execution_side_to_intent_side(fill.side);
        let candidates = self
            .intents
            .find_open_for_fill(&fill.account, &fill.symbol, side)
            .await?;
        let Some(decision) = match_fill(fill, &candidates) else {
            return Ok(None);
        };
        let updated = self.intents.apply_linkage(decision.clone()).await?;
        if !updated {
            debug!(
                exec_id = %fill.exec_id,
                "tca attach_fill: executions row missing or already linked"
            );
            return Ok(None);
        }
        Ok(Some(decision))
    }

    /// Sweep the day's executions for the given account and try to
    /// link each. Idempotent — fills already linked (intent_id IS NOT
    /// NULL) are skipped by the UPDATE filter inside
    /// `OrderIntentStore::apply_linkage`. Returns the number of
    /// freshly-attached fills.
    pub async fn attach_fills_for_account_today(&self, account: &str) -> Result<usize> {
        let today_et = Utc::now()
            .with_timezone(&chrono_tz::America::New_York)
            .date_naive();
        self.attach_fills_for_account_date(account, today_et).await
    }

    /// Sweep a specific ET trading date. Used by tests + future
    /// backfill commands.
    pub async fn attach_fills_for_account_date(
        &self,
        account: &str,
        date: NaiveDate,
    ) -> Result<usize> {
        let fills = self.executions.query(account, date, None).await?;
        let mut n = 0;
        for fill in &fills {
            // Best-effort per fill — a single match failure shouldn't
            // poison the rest of the batch.
            match self.attach_fill(fill).await {
                Ok(Some(_)) => n += 1,
                Ok(None) => {}
                Err(e) => {
                    warn!(exec_id = %fill.exec_id, error = %e, "tca attach_fill failed");
                }
            }
        }
        Ok(n)
    }

    /// Mark any open intent whose window has elapsed as `expired`.
    /// Run alongside the executions ingestor.
    pub async fn expire_stale(&self) -> Result<usize> {
        Ok(self.intents.expire_stale(Utc::now()).await?)
    }
}
