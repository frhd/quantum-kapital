// Phase 9 — public-surface helpers (Tauri commands, frontend serde,
// gate descriptor) that the lib doesn't yet call internally. Kept on
// the API rather than gated behind feature flags so external consumers
// land cleanly.
#![allow(dead_code, unused_imports)]

//! Phase 9 (quant-decisions) — `RegimeService`: deterministic regime
//! classifier + per-detector preferred-regime gate. The runner consults
//! this between blackout and concentration gates and skips off-regime
//! hits with `skipped_reason = 'off_regime'`.
//!
//! Three deterministic surfaces:
//!
//!   - [`RegimeService::snapshot`] — recompute current `Regime` from
//!     the bars-cache inputs, persist a `regime_snapshots` row, cache
//!     it as `latest`, emit `RegimeChanged` if the stable view flips.
//!   - [`RegimeService::current`] — cheap reader for the live (or
//!     freshly-computed) cached snapshot.
//!   - [`RegimeService::evaluate`] — gate descriptor for one detector:
//!     "given the current stable regime + this detector's preferred
//!     regimes, does the candidate pass?".
//!
//! Mock-friendly: the bars seam is the backtester's `BarsReader` trait,
//! same one P6 already mocks. No IBKR-touching code in this module.
//!
//! Persistence rule: the 3-day per-axis flip rule from the phase doc
//! applies to the stable view only. Raw classifications land in
//! `regime_snapshots` every time `snapshot` runs (daily-close +
//! intraday + force-recompute); the stable view is recomputed at gate
//! time from the most recent N daily-close rows + the new raw read.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use thiserror::Error;
use tokio::sync::{Mutex, RwLock};

use crate::events::{AppEvent, EventEmitter};
use crate::services::backtester::bars_reader::BarsReader;
use crate::storage::error::StorageError;
use crate::storage::Db;

mod classifier;
mod config;
mod inputs;
mod snapshot_store;
mod types;

#[cfg(test)]
mod tests;

pub use config::RegimeConfig;
pub use inputs::{
    BreadthInputs, CorrInputs, InputGatherer, RegimeInputs, SpyInputs, VixInputs, UNIVERSE,
};
pub use snapshot_store::{RegimeSnapshotRow, SnapshotStore};
pub use types::{BreadthAxis, CorrAxis, Regime, RegimeFilter, SnapshotSource, TrendAxis, VolAxis};

#[derive(Error, Debug)]
pub enum RegimeError {
    #[error("storage: {0}")]
    Storage(#[from] StorageError),
    #[error("serde_json: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, RegimeError>;

/// Outcome of a per-detector gate evaluation. `matches = true` means
/// the candidate passes; the runner threads the descriptor into both
/// the persisted skip-row's `skip_window_json` (when blocked) and the
/// fired-row's `regime_at_decision_json` (when passed) so the audit
/// can replay the gate decision.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GateDecision {
    pub matches: bool,
    pub regime: Regime,
    pub preferred: RegimeFilter,
    pub stale: bool,
    pub source: SnapshotSource,
}

/// Phase 9 entry point. Cheap to clone (`Arc` internals).
#[derive(Clone)]
pub struct RegimeService {
    db: Arc<Db>,
    emitter: Arc<EventEmitter>,
    gatherer: Arc<InputGatherer>,
    snapshot_store: SnapshotStore,
    config: Arc<RwLock<RegimeConfig>>,
    /// Single-flight guard so a burst of triggers (15-min tick + a
    /// detector-firing recompute) collapses into one snapshot.
    recompute_guard: Arc<Mutex<()>>,
    /// Cached most-recent stable-view snapshot. None until the first
    /// `snapshot()` call lands.
    latest: Arc<RwLock<Option<CachedSnapshot>>>,
}

/// Combined raw + stable view of the most recent classification.
#[derive(Debug, Clone)]
pub struct CachedSnapshot {
    pub at: DateTime<Utc>,
    pub raw: Regime,
    pub stable: Regime,
    pub inputs: RegimeInputs,
    pub source: SnapshotSource,
    pub snapshot_id: i64,
}

impl RegimeService {
    pub fn new(
        db: Arc<Db>,
        emitter: Arc<EventEmitter>,
        bars: Arc<dyn BarsReader>,
        config: RegimeConfig,
    ) -> Self {
        Self {
            db: Arc::clone(&db),
            emitter,
            gatherer: Arc::new(InputGatherer::new(bars)),
            snapshot_store: SnapshotStore::new(db),
            config: Arc::new(RwLock::new(config)),
            recompute_guard: Arc::new(Mutex::new(())),
            latest: Arc::new(RwLock::new(None)),
        }
    }

    /// Construct with a custom `InputGatherer` (e.g. tests want to
    /// pin the universe to the canned-bars set).
    pub fn with_gatherer(
        db: Arc<Db>,
        emitter: Arc<EventEmitter>,
        gatherer: Arc<InputGatherer>,
        config: RegimeConfig,
    ) -> Self {
        Self {
            db: Arc::clone(&db),
            emitter,
            gatherer,
            snapshot_store: SnapshotStore::new(db),
            config: Arc::new(RwLock::new(config)),
            recompute_guard: Arc::new(Mutex::new(())),
            latest: Arc::new(RwLock::new(None)),
        }
    }

    pub async fn config(&self) -> RegimeConfig {
        self.config.read().await.clone()
    }

    pub async fn set_config(&self, cfg: RegimeConfig) {
        *self.config.write().await = cfg;
    }

    /// Recompute the regime snapshot. Persists a `regime_snapshots`
    /// row, applies the 3-day persistence rule against the most
    /// recent daily-close history, caches the stable view as
    /// `latest`, and emits `RegimeChanged` only when the stable view
    /// actually flips on at least one axis.
    pub async fn snapshot(&self, source: SnapshotSource) -> Result<CachedSnapshot> {
        let _guard = self.recompute_guard.lock().await;

        let now = Utc::now();
        let inputs = self.gatherer.gather(now).await?;
        let raw = classifier::classify(&inputs);

        // Persistence rule: read the 4 most recent daily-close rows
        // BEFORE inserting the new raw. The prior stable is the
        // last row's persisted `stable` field (NOT raw), so the rule
        // compares today's raw against the long-running stable view.
        let prior_history = self
            .snapshot_store
            .list_by_source(SnapshotSource::DailyClose, 4)
            .await?;
        let prior_stable = prior_history.first().map(|row| row.stable).unwrap_or(raw);
        // Persistence rule reads PRIOR raws — the new `raw` is today's
        // and is excluded by construction (priors loaded before insert).
        let prior_raws: Vec<Regime> = prior_history.iter().map(|row| row.raw).collect();
        let stable = if matches!(source, SnapshotSource::DailyClose) {
            Regime::apply_persistence(prior_stable, raw, &prior_raws)
        } else {
            // Intraday + force-recompute don't trigger the 3-day rule;
            // they observe the prior stable view. The raw classification
            // still lands in the row for audit.
            prior_stable
        };

        let snapshot_id = self
            .snapshot_store
            .insert(now, &raw, &stable, &inputs, source)
            .await?;

        let cached = CachedSnapshot {
            at: now,
            raw,
            stable,
            inputs,
            source,
            snapshot_id,
        };

        let prev = self.latest.write().await.replace(cached.clone());
        let flipped = prev
            .as_ref()
            .map(|p| p.stable != cached.stable)
            .unwrap_or(true);
        if flipped {
            let _ = self
                .emitter
                .emit(AppEvent::RegimeChanged {
                    snapshot_id: cached.snapshot_id,
                    regime: cached.stable,
                    source: source.as_str().to_string(),
                })
                .await;
        }

        Ok(cached)
    }

    /// Cheap reader. Returns the cached snapshot if present, otherwise
    /// recomputes via `snapshot(SnapshotSource::ForceRecompute)`.
    pub async fn current(&self) -> Result<CachedSnapshot> {
        if let Some(s) = self.latest.read().await.clone() {
            return Ok(s);
        }
        self.snapshot(SnapshotSource::ForceRecompute).await
    }

    /// Read the most-recent `limit` snapshot rows, newest-first.
    pub async fn history(&self, limit: u32) -> Result<Vec<RegimeSnapshotRow>> {
        self.snapshot_store.list(limit).await
    }

    /// Per-detector gate evaluation. Returns a [`GateDecision`] the
    /// runner threads into the persisted skip / fired row so the
    /// audit can replay the gate. When the config is disabled, every
    /// detector passes with `matches: true`.
    pub async fn evaluate(&self, detector_name: &str) -> Result<GateDecision> {
        let cfg = self.config().await;
        let snapshot = self.current().await?;
        let preferred = cfg.filter_for(detector_name);
        // When the gate is globally disabled, return `matches: true`
        // with an empty filter so the audit row reads as a clean pass.
        let matches = if cfg.enabled {
            preferred.matches(&snapshot.stable)
        } else {
            true
        };
        let stale = snapshot
            .inputs
            .missing
            .iter()
            .any(|m| m.starts_with("spy") || m.starts_with("breadth") || m.starts_with("corr"));
        Ok(GateDecision {
            matches,
            regime: snapshot.stable,
            preferred,
            stale,
            source: snapshot.source,
        })
    }

    /// Audit a regime-gate override. Mirrors the `record_override`
    /// path on `PortfolioRiskService` (V24 unified `gate_overrides`
    /// table).
    pub async fn record_override(&self, setup_id: i64, reason: &str, actor: &str) -> Result<i64> {
        let kind = "regime".to_string();
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

impl std::fmt::Debug for RegimeService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RegimeService").finish_non_exhaustive()
    }
}
