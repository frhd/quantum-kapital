//! Phase 8 — `PortfolioRiskService` integration tests. These walk
//! through the full happy path against a real SQLite DB + the
//! `EquitySnapshotService`, with a stub `OpenPositionsSource` to
//! avoid a live IBKR client.

#![allow(clippy::too_many_arguments)]

use std::sync::Arc;

use async_trait::async_trait;
use tempfile::NamedTempFile;
use tokio::sync::Mutex;

use crate::events::EventEmitter;
use crate::ibkr::error::IbkrError;
use crate::ibkr::types::positions::Position;
use crate::services::risk_engine::{
    AccountSource, EquityFetcher, EquitySnapshotService,
};
use crate::storage::Db;

use super::concentration_gate::{ConcentrationConfig, GateInput, GateSeverity};
use super::factors::FactorBuckets;
use super::sector_map::SectorMap;
use super::{OpenPositionsSource, PortfolioRiskService};

struct FixedAccount(&'static str);

#[async_trait]
impl AccountSource for FixedAccount {
    async fn current_account(&self) -> Result<String, IbkrError> {
        Ok(self.0.to_string())
    }
}

struct StubEquity {
    nlv: f64,
}

#[async_trait]
impl EquityFetcher for StubEquity {
    async fn fetch_nlv(&self, _account: &str) -> Result<f64, IbkrError> {
        Ok(self.nlv)
    }
}

struct StubPositions {
    rows: Mutex<Vec<Position>>,
}

#[async_trait]
impl OpenPositionsSource for StubPositions {
    async fn list_open(&self, _account: &str) -> Result<Vec<Position>, IbkrError> {
        Ok(self.rows.lock().await.clone())
    }
}

fn pos(symbol: &str, qty: f64, avg: f64) -> Position {
    Position {
        symbol: symbol.to_string(),
        position: qty,
        average_cost: avg,
        ..Default::default()
    }
}

async fn build_service(
    nlv: f64,
    positions: Vec<Position>,
) -> (NamedTempFile, Arc<PortfolioRiskService>) {
    let tmp = NamedTempFile::new().unwrap();
    let db = Arc::new(Db::open(tmp.path()).unwrap());
    let fetcher: Arc<dyn EquityFetcher> = Arc::new(StubEquity { nlv });
    let equity = Arc::new(EquitySnapshotService::new(Arc::clone(&db), fetcher));
    let positions: Arc<dyn OpenPositionsSource> = Arc::new(StubPositions {
        rows: Mutex::new(positions),
    });
    let account: Arc<dyn AccountSource> = Arc::new(FixedAccount("DU1"));
    let emitter = Arc::new(EventEmitter::for_capture());
    let svc = Arc::new(PortfolioRiskService::new(
        db,
        positions,
        account,
        equity,
        emitter,
        SectorMap::arc(),
        FactorBuckets::arc(),
        ConcentrationConfig::default(),
    ));
    (tmp, svc)
}

#[tokio::test]
async fn snapshot_persists_and_emits() {
    let (_tmp, svc) =
        build_service(100_000.0, vec![pos("NVDA", 100.0, 100.0)]).await;
    let s = svc.snapshot().await.unwrap();
    assert_eq!(s.account, "DU1");
    assert_eq!(s.nlv_cents, 10_000_000);
    assert_eq!(s.open_positions.len(), 1);
    assert!(s.snapshot_id > 0);

    // History reads it back.
    let hist = svc.history(10).await.unwrap();
    assert_eq!(hist.len(), 1);
    assert_eq!(hist[0].id, s.snapshot_id);
    assert_eq!(hist[0].nlv_cents, 10_000_000);
}

#[tokio::test]
async fn gate_against_empty_portfolio_passes_small_candidate() {
    // Empty portfolio + small $50 candidate → all limits comfortable.
    // Severity-ladder edge cases live in `concentration_gate::tests`;
    // this test pins that the live wiring serves a usable gate.
    let (_tmp, svc) = build_service(100_000.0, vec![]).await;
    let _ = svc.snapshot().await.unwrap();
    let gate = svc.gate().await.unwrap();
    let r = gate.check_with_sector(
        &GateInput {
            symbol: "NVDA",
            projected_dollar_risk_cents: 5_000,
            strategy: "breakout",
            momentum_bucket: None,
        },
        Some("semis"),
    );
    assert_eq!(r.severity, GateSeverity::Pass);
}

#[tokio::test]
async fn override_audit_writes_gate_override_row() {
    let (_tmp, svc) = build_service(100_000.0, vec![]).await;
    let _ = svc.snapshot().await.unwrap();
    // Insert a setup so the FK is satisfiable.
    let setup_id = svc
        .config()
        .await
        .clone(); // touch the config to avoid unused-warn
    let _ = setup_id;

    // Create a setups row directly so the override FK satisfies.
    let conn_rows = svc
        .clone()
        .record_override(
            insert_dummy_setup(&svc).await,
            "concentration",
            "trader override: low correlation in this slice",
            "human",
        )
        .await
        .unwrap();
    assert!(conn_rows > 0);
}

async fn insert_dummy_setup(svc: &PortfolioRiskService) -> i64 {
    // Hand-roll a minimal `tracked_tickers` parent + `setups` child so
    // the override FK satisfies. Bypasses TrackerService because the
    // test only needs an id.
    let id = svc
        .db
        .with_conn(|conn| {
            conn.execute(
                "INSERT INTO tracked_tickers \
                   (symbol, source, status, tags, added_at) \
                 VALUES ('NVDA', 'manual', 'watching', '[]', 0)",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO setups \
                  (symbol, strategy, direction, detected_at, trigger_price, stop_price, \
                   targets, raw_signals, status) \
                 VALUES ('NVDA', 'breakout', 'long', 0, 100.0, 95.0, '[]', '{}', 'active')",
                [],
            )
            .unwrap();
            Ok(conn.last_insert_rowid())
        })
        .await
        .unwrap();
    id
}

/// Master cross-phase verification: 100 rapid recompute calls do not
/// corrupt the cache (single-flight via the recompute_guard mutex).
#[tokio::test]
async fn concurrent_recompute_burst_yields_consistent_cache() {
    let (_tmp, svc) =
        build_service(100_000.0, vec![pos("NVDA", 100.0, 100.0)]).await;

    let mut joinset = tokio::task::JoinSet::new();
    for _ in 0..50 {
        let s = Arc::clone(&svc);
        joinset.spawn(async move { s.snapshot().await.unwrap().total_dollar_risk_cents });
    }
    let mut totals = Vec::new();
    while let Some(res) = joinset.join_next().await {
        totals.push(res.unwrap());
    }
    // All 50 reads must agree (positions never changed).
    assert!(totals.windows(2).all(|w| w[0] == w[1]));
    assert_eq!(totals[0], 50_000); // 100 sh × $5 fallback risk
}
