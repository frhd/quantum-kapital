//! Phase 1 — `RiskEngine`: the deterministic position-sizer.
//!
//! Wires the pure `compute_sizing` math against a real
//! `EquitySnapshotService`, a configurable `RiskConfig` (settable
//! at runtime via `risk_set_config`), and an `AccountSource` so
//! callers don't need to thread the account name. `size` returns
//! a `Sizing` struct ready to land on the `setups` row.
//!
//! The engine is the only path to sizing in this codebase. Direct
//! calls to `compute_sizing` exist only in tests; production
//! services (tracker_runner, recompute commands) all go through
//! `RiskEngine`.

use std::sync::Arc;

use async_trait::async_trait;
use thiserror::Error;
use tokio::sync::RwLock;
use tracing::warn;

use crate::ibkr::client::IbkrClient;
use crate::ibkr::error::IbkrError;
use crate::strategies::SetupCandidate;

mod equity_snapshot;
mod sizing;
mod types;

pub use equity_snapshot::{EquityFetcher, EquitySnapshotError, EquitySnapshotService};
pub use sizing::compute_sizing;
// `EquitySource` is part of `EquitySnapshot`'s public shape; the
// re-export keeps callers outside this module from having to dig
// into the private `types` submodule.
#[allow(unused_imports)]
pub use types::{
    ConvictionGrade, EquitySnapshot, EquitySource, RiskConfig, Sizing, SizingSkippedReason,
};

/// Trait seam for "which IBKR account does sizing pin to?". The
/// default policy picks the first account `IbkrClient::get_accounts`
/// returns; if a future multi-account UI ships, callers swap in a
/// per-user resolver. Tests inject a fixed-account stub.
#[async_trait]
pub trait AccountSource: Send + Sync {
    async fn current_account(&self) -> std::result::Result<String, IbkrError>;
}

#[async_trait]
impl AccountSource for IbkrClient {
    async fn current_account(&self) -> std::result::Result<String, IbkrError> {
        let accounts = self.get_accounts().await?;
        accounts
            .into_iter()
            .next()
            .ok_or_else(|| IbkrError::RequestFailed("no IBKR accounts available".to_string()))
    }
}

#[derive(Error, Debug)]
pub enum RiskEngineError {
    #[error("ibkr: {0}")]
    Ibkr(#[from] IbkrError),
    #[error("equity snapshot: {0}")]
    Snapshot(#[from] EquitySnapshotError),
}

pub type Result<T> = std::result::Result<T, RiskEngineError>;

/// The sizing engine. Cheap to clone (`Arc` internals) so it can be
/// `app.manage`'d once and pulled as `State<Arc<RiskEngine>>` from
/// every command.
#[derive(Clone)]
pub struct RiskEngine {
    snapshot_svc: Arc<EquitySnapshotService>,
    account_source: Arc<dyn AccountSource>,
    config: Arc<RwLock<RiskConfig>>,
}

impl RiskEngine {
    pub fn new(
        snapshot_svc: Arc<EquitySnapshotService>,
        account_source: Arc<dyn AccountSource>,
        config: RiskConfig,
    ) -> Self {
        Self {
            snapshot_svc,
            account_source,
            config: Arc::new(RwLock::new(config)),
        }
    }

    /// Read a clone of the live config. Hot-reload safe — the
    /// returned struct is a snapshot, mutating the engine after
    /// this point doesn't affect the caller's copy.
    pub async fn config(&self) -> RiskConfig {
        self.config.read().await.clone()
    }

    /// Replace the live config. New sizings written from this point
    /// forward use the new struct; previously-persisted setups stay
    /// at their `sizing_version`.
    pub async fn set_config(&self, cfg: RiskConfig) {
        *self.config.write().await = cfg;
    }

    /// Resolve the active account, fetch its current equity snapshot,
    /// and run `compute_sizing`. Snapshot lookup follows the
    /// fresh-then-stale fallback policy in `EquitySnapshotService`.
    /// Returns the `EquitySnapshot` alongside `Sizing` so callers
    /// (commands, runner) can persist and emit it without a second
    /// DB hop.
    pub async fn size_for_candidate(
        &self,
        candidate: &SetupCandidate,
    ) -> Result<(Sizing, EquitySnapshot)> {
        let account = self.account_source.current_account().await?;
        let snapshot = self.snapshot_svc.current(&account).await?;
        let cfg = self.config().await;
        let sizing = compute_sizing(candidate, &snapshot, &cfg);
        if let Some(reason) = sizing.skipped_reason {
            warn!(
                "risk_engine: skipped sizing for {} ({:?}) — equity={}c, grade={:?}",
                candidate.strategy, reason, snapshot.nlv_cents, sizing.conviction_grade,
            );
        }
        Ok((sizing, snapshot))
    }

    /// Force-refresh the equity snapshot from IBKR (used after a
    /// deposit / withdrawal). Returns the new persisted row.
    pub async fn refresh_equity(&self) -> Result<EquitySnapshot> {
        let account = self.account_source.current_account().await?;
        Ok(self.snapshot_svc.force_refresh(&account).await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use serde_json::json;
    use std::sync::Mutex;
    use tempfile::NamedTempFile;

    use crate::ibkr::types::{BarSize, StrategyTag};
    use crate::storage::Db;
    use crate::strategies::Direction;

    struct FixedAccount(&'static str);

    #[async_trait]
    impl AccountSource for FixedAccount {
        async fn current_account(&self) -> std::result::Result<String, IbkrError> {
            Ok(self.0.to_string())
        }
    }

    struct StubFetcher {
        nlv: Mutex<f64>,
    }

    #[async_trait]
    impl EquityFetcher for StubFetcher {
        async fn fetch_nlv(&self, _account: &str) -> std::result::Result<f64, IbkrError> {
            Ok(*self.nlv.lock().unwrap())
        }
    }

    fn engine_with_nlv(nlv: f64) -> (NamedTempFile, RiskEngine) {
        let tmp = NamedTempFile::new().unwrap();
        let db = Arc::new(Db::open(tmp.path()).unwrap());
        let fetcher: Arc<dyn EquityFetcher> = Arc::new(StubFetcher {
            nlv: Mutex::new(nlv),
        });
        let snap_svc = Arc::new(EquitySnapshotService::new(db, fetcher));
        let engine = RiskEngine::new(
            snap_svc,
            Arc::new(FixedAccount("DU1")),
            RiskConfig::default(),
        );
        (tmp, engine)
    }

    fn cand(signal: f64, trigger: f64, stop: f64) -> SetupCandidate {
        SetupCandidate {
            strategy: "breakout",
            tag: StrategyTag::Breakout,
            direction: Direction::Long,
            conviction_signal: signal,
            trigger_price: trigger,
            stop_price: stop,
            targets: Vec::new(),
            raw_signals: json!({}),
            timeframe: BarSize::Day1,
            detected_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn size_for_candidate_returns_a_grade_sizing() {
        let (_tmp, engine) = engine_with_nlv(100_000.0);
        let (sizing, snap) = engine
            .size_for_candidate(&cand(0.9, 105.0, 100.0))
            .await
            .unwrap();
        assert_eq!(sizing.qty, 100);
        assert_eq!(sizing.conviction_grade, ConvictionGrade::A);
        assert_eq!(snap.account, "DU1");
        assert_eq!(snap.source, EquitySource::IbkrAccountSummary);
    }

    #[tokio::test]
    async fn config_updates_take_effect_immediately() {
        let (_tmp, engine) = engine_with_nlv(100_000.0);
        let mut cfg = engine.config().await;
        cfg.risk_pct_a = 0.01; // 1% — double the default.
        engine.set_config(cfg).await;
        let (sizing, _) = engine
            .size_for_candidate(&cand(0.9, 105.0, 100.0))
            .await
            .unwrap();
        // 1% * 100k / $5 = 200 sh.
        assert_eq!(sizing.qty, 200);
    }

    #[tokio::test]
    async fn snapshot_is_cached_across_size_calls() {
        let (_tmp, engine) = engine_with_nlv(100_000.0);
        let (s1, snap1) = engine
            .size_for_candidate(&cand(0.9, 105.0, 100.0))
            .await
            .unwrap();
        let (s2, snap2) = engine
            .size_for_candidate(&cand(0.9, 110.0, 105.0))
            .await
            .unwrap();
        // Same as_of_date implies same trading day cache hit.
        assert_eq!(snap1.as_of_date, snap2.as_of_date);
        assert_eq!(s1.equity_at_decision_cents, s2.equity_at_decision_cents);
    }
}
