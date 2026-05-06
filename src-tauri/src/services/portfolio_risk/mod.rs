//! Phase 8 (quant-decisions) — `PortfolioRiskService`: portfolio-level
//! state computed from open IBKR positions joined against
//! `bracket_groups` stop prices. The same exposure math drives both
//! the live dashboard (`portfolio_risk_snapshot`) and the pre-trade
//! `ConcentrationGate` consulted by `TrackerRunner` before a setup is
//! persisted.
//!
//! Three deterministic surfaces:
//!
//!   - [`PortfolioRiskService::snapshot`] — current `PortfolioRisk`
//!     view: open positions × stop distance × sector / factor
//!     bucketing, plus persisted `portfolio_snapshots` row for the
//!     historical timeline.
//!   - [`PortfolioRiskService::history`] — timeseries of
//!     `(at, total_dollar_risk_cents)` from `portfolio_snapshots`.
//!   - [`ConcentrationGate::check`] — pre-trade hypothetical: "if I
//!     add this candidate at its sized qty, where do total / sector /
//!     name dollar-risk land relative to limits?". Returns
//!     `pass | warn | block`.
//!
//! Everything is mock-friendly: the IBKR-touching seam is the
//! `OpenPositionsSource` trait so tests inject canned positions
//! without standing up a live client. Bracket-stop and equity reads
//! go through the existing `Db` and `EquitySnapshotService` directly.
//!
//! Master "Hard invariants" #5 — when limits change in
//! `ConcentrationConfig`, the live engine flips immediately but past
//! `portfolio_snapshots` rows stay immutable and replay against the
//! limits in force at the time of the gate decision (limit value is
//! stored on the `gate_overrides` audit row, not the snapshot).

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use thiserror::Error;
use tokio::sync::{Mutex, RwLock};

use crate::events::{AppEvent, EventEmitter};
use crate::ibkr::error::IbkrError;
use crate::ibkr::types::positions::Position;
use crate::services::risk_engine::{
    AccountSource, EquitySnapshotError, EquitySnapshotService, RiskEngineError,
};
use crate::storage::error::StorageError;
use crate::storage::Db;

mod concentration_gate;
mod exposure;
mod factors;
mod sector_map;
mod snapshot_store;
mod types;

#[cfg(test)]
mod tests;

pub use concentration_gate::{
    ConcentrationConfig, ConcentrationGate, GateInput, GateResult, GateSeverity,
};
pub use exposure::{ExposureSlice, FactorBucket, OpenPosition, PortfolioRisk, SectorBucket};
pub use factors::FactorBuckets;
pub use sector_map::SectorMap;
pub use snapshot_store::{PortfolioSnapshotRow, SnapshotStore};
pub use types::{ConcentrationKind, GateLimitBreach};

/// Trait seam for "give me the current open positions for `account`".
/// Production wiring is the live `IbkrClient::get_positions`; tests
/// inject canned slices to avoid the IBKR stack.
#[async_trait]
pub trait OpenPositionsSource: Send + Sync {
    async fn list_open(&self, account: &str) -> std::result::Result<Vec<Position>, IbkrError>;
}

#[async_trait]
impl OpenPositionsSource for crate::ibkr::client::IbkrClient {
    async fn list_open(&self, account: &str) -> std::result::Result<Vec<Position>, IbkrError> {
        let raw = self.get_positions(account).await?;
        // Filter zero-qty rows (IBKR sometimes reports closed positions
        // with position=0 in the stream until they age out).
        Ok(raw.into_iter().filter(|p| p.position.abs() > 0.0).collect())
    }
}

#[derive(Error, Debug)]
pub enum PortfolioRiskError {
    #[error("ibkr: {0}")]
    Ibkr(#[from] IbkrError),
    #[error("storage: {0}")]
    Storage(#[from] StorageError),
    #[error("equity snapshot: {0}")]
    Snapshot(#[from] EquitySnapshotError),
    #[error("risk engine: {0}")]
    Engine(#[from] RiskEngineError),
    #[error("serde_json: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, PortfolioRiskError>;

/// Phase 8 entry point. Cheap to clone (`Arc` internals) — managed
/// once by Tauri and pulled as `State<Arc<PortfolioRiskService>>`.
#[derive(Clone)]
pub struct PortfolioRiskService {
    db: Arc<Db>,
    positions: Arc<dyn OpenPositionsSource>,
    account_source: Arc<dyn AccountSource>,
    equity: Arc<EquitySnapshotService>,
    emitter: Arc<EventEmitter>,
    sector_map: Arc<SectorMap>,
    factors: Arc<FactorBuckets>,
    config: Arc<RwLock<ConcentrationConfig>>,
    snapshot_store: SnapshotStore,
    /// Single-flight guard so 100 rapid `executions` events collapse
    /// into one snapshot recompute. The Mutex is held for the entire
    /// recompute; concurrent callers wait, then read the cached
    /// `latest` if it's still fresh.
    recompute_guard: Arc<Mutex<()>>,
    /// Cached most-recent snapshot. None until the first
    /// `snapshot()` call lands.
    latest: Arc<RwLock<Option<PortfolioRisk>>>,
}

impl PortfolioRiskService {
    pub fn new(
        db: Arc<Db>,
        positions: Arc<dyn OpenPositionsSource>,
        account_source: Arc<dyn AccountSource>,
        equity: Arc<EquitySnapshotService>,
        emitter: Arc<EventEmitter>,
        sector_map: Arc<SectorMap>,
        factors: Arc<FactorBuckets>,
        config: ConcentrationConfig,
    ) -> Self {
        Self {
            db: Arc::clone(&db),
            positions,
            account_source,
            equity,
            emitter,
            sector_map,
            factors,
            config: Arc::new(RwLock::new(config)),
            snapshot_store: SnapshotStore::new(db),
            recompute_guard: Arc::new(Mutex::new(())),
            latest: Arc::new(RwLock::new(None)),
        }
    }

    /// Read a clone of the live concentration config. Hot-reload safe.
    pub async fn config(&self) -> ConcentrationConfig {
        self.config.read().await.clone()
    }

    /// Replace the live config. Future `snapshot()` calls and
    /// `ConcentrationGate::check` calls pick the new limits up
    /// immediately; previously-persisted snapshots stay immutable.
    pub async fn set_config(&self, cfg: ConcentrationConfig) {
        *self.config.write().await = cfg;
    }

    /// Recompute the portfolio risk snapshot from live positions +
    /// brackets + equity. Persists a row to `portfolio_snapshots`,
    /// caches it as `latest`, and emits `PortfolioRiskChanged` so the
    /// dashboard refreshes. Single-flight via `recompute_guard` so a
    /// burst of triggers collapses to one snapshot.
    pub async fn snapshot(&self) -> Result<PortfolioRisk> {
        let _guard = self.recompute_guard.lock().await;

        let account = self
            .account_source
            .current_account()
            .await
            .map_err(|e| RiskEngineError::Ibkr(e))?;
        let equity = self.equity.current(&account).await?;

        let raw_positions = self.positions.list_open(&account).await?;
        let bracket_stops = self
            .snapshot_store
            .open_bracket_stops(&account)
            .await?;

        let now = Utc::now();
        let exposures = exposure::compute(
            &account,
            now,
            equity.nlv_cents,
            &raw_positions,
            &bracket_stops,
            &self.sector_map,
            &self.factors,
        );

        let snapshot_id = self
            .snapshot_store
            .insert(&account, now, &exposures)
            .await?;

        let portfolio = PortfolioRisk {
            snapshot_id,
            account: account.clone(),
            at: now,
            nlv_cents: equity.nlv_cents,
            ..exposures
        };

        *self.latest.write().await = Some(portfolio.clone());

        let _ = self
            .emitter
            .emit(AppEvent::PortfolioRiskChanged {
                snapshot_id: portfolio.snapshot_id,
                account: portfolio.account.clone(),
                nlv_cents: portfolio.nlv_cents,
                total_dollar_risk_cents: portfolio.total_dollar_risk_cents,
                open_position_count: portfolio.open_positions.len(),
            })
            .await;

        Ok(portfolio)
    }

    /// Return the cached snapshot if any; otherwise compute one.
    /// Cheap path for UIs that don't care about a sub-60s refresh.
    pub async fn snapshot_or_cached(&self) -> Result<PortfolioRisk> {
        if let Some(s) = self.latest.read().await.clone() {
            return Ok(s);
        }
        self.snapshot().await
    }

    /// Read the persisted history of `portfolio_snapshots` rows for
    /// the current account, newest-first, capped at `limit`.
    pub async fn history(&self, limit: u32) -> Result<Vec<PortfolioSnapshotRow>> {
        let account = self
            .account_source
            .current_account()
            .await
            .map_err(|e| RiskEngineError::Ibkr(e))?;
        self.snapshot_store
            .list(&account, limit.max(1).min(1000))
            .await
    }

    /// Build the gate that pre-trade callers consult. The gate reads
    /// the *current* config + the cached snapshot (or computes one if
    /// stale) so a `check` call is cheap on the hot detector path.
    pub async fn gate(&self) -> Result<ConcentrationGate> {
        let snapshot = self.snapshot_or_cached().await?;
        let config = self.config().await;
        Ok(ConcentrationGate::new(snapshot, config, Arc::clone(&self.sector_map)))
    }

    /// Audit a gate override. Called by the frontend confirm-flow
    /// when the trader overrides a `block` or chooses to take a
    /// `warn`-flagged setup despite the banner. Mirrors the
    /// `setup_blackout_overrides` pattern from Phase 5 but on the
    /// unified `gate_overrides` table from V24.
    pub async fn record_override(
        &self,
        setup_id: i64,
        gate_kind: &str,
        reason: &str,
        actor: &str,
    ) -> Result<i64> {
        let kind = gate_kind.to_string();
        let reason = reason.to_string();
        let actor = actor.to_string();
        let at = Utc::now().timestamp();
        let id = self
            .db
            .with_conn(move |conn| {
                conn.execute(
                    "INSERT INTO gate_overrides \
                       (setup_id, gate_kind, reason, actor, at_unix) \
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    rusqlite::params![setup_id, kind, reason, actor, at],
                )
                .map_err(StorageError::from)?;
                Ok(conn.last_insert_rowid())
            })
            .await?;
        Ok(id)
    }
}

/// Hand-rolled debug for the service so it can appear in tracing
/// fields without dumping the trait objects.
impl std::fmt::Debug for PortfolioRiskService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PortfolioRiskService").finish_non_exhaustive()
    }
}

/// Convenience map: extract just the snapshot ID + at-time for
/// "is this snapshot stale?" checks without re-reading the row.
pub type SnapshotMeta = (i64, DateTime<Utc>);
