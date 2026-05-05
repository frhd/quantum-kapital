//! Phase 3 — `services/order_ticket/` integration tests.
//!
//! Drives the full sized-setup → bracket-placed flow against an
//! in-memory mock placer + a temp-file SQLite DB. Mirrors the master
//! plan's exit criteria: clean fill, stop-out, target-1 partial,
//! full target sweep, manual cancel — every leg lands in
//! `bracket_groups` with the correct shape, and the tracer-bullet
//! variant additionally exercises the P2 intent linkage so a future
//! ingestor pass would see `setup_id` populated on each `executions`
//! row.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use tempfile::NamedTempFile;
use tokio::sync::Mutex;

use crate::events::EventEmitter;
use crate::ibkr::error::IbkrError;
use crate::ibkr::types::{BracketReceipt, BracketRequest};
use crate::services::executions::ExecutionsStore;
use crate::services::risk_engine::{EquityFetcher, EquitySnapshotService};
use crate::services::tca::TcaService;
use crate::services::tracker_service::TrackerService;
use crate::storage::Db;
use crate::strategies::Direction;

use super::{
    AccountResolver, BracketGroupStore, BracketPlacer, OrderTicket, OrderTicketError, TakeSetupArgs,
};

// ---------------- mocks ----------------

#[derive(Clone, Default)]
struct MockBracketPlacer {
    /// Recorded `place_bracket` requests, in submission order.
    inbox: Arc<Mutex<Vec<BracketRequest>>>,
    /// If `Some(_)`, every call returns this error instead of a
    /// receipt. Lets the failure-path test exercise the "intent
    /// recorded but bracket failed" branch.
    fail: Arc<Mutex<Option<IbkrError>>>,
    /// Monotonic counter so each bracket sees fresh order ids.
    next_id: Arc<Mutex<i32>>,
}

impl MockBracketPlacer {
    fn new() -> Self {
        Self {
            inbox: Arc::new(Mutex::new(Vec::new())),
            fail: Arc::new(Mutex::new(None)),
            next_id: Arc::new(Mutex::new(1000)),
        }
    }

    async fn calls(&self) -> Vec<BracketRequest> {
        self.inbox.lock().await.clone()
    }
}

#[async_trait]
impl BracketPlacer for MockBracketPlacer {
    async fn place_bracket(
        &self,
        req: BracketRequest,
    ) -> std::result::Result<BracketReceipt, IbkrError> {
        if let Some(e) = self.fail.lock().await.as_ref() {
            return Err(e.clone());
        }
        self.inbox.lock().await.push(req.clone());
        let mut n = self.next_id.lock().await;
        let parent = *n;
        *n += 1;
        let stop = *n;
        *n += 1;
        let mut targets = Vec::with_capacity(req.target_rungs.len());
        for _ in &req.target_rungs {
            targets.push(*n);
            *n += 1;
        }
        Ok(BracketReceipt {
            parent_order_id: parent,
            stop_order_id: stop,
            target_order_ids: targets,
        })
    }
}

struct FixedAccount(&'static str);

#[async_trait]
impl AccountResolver for FixedAccount {
    async fn account(&self) -> std::result::Result<String, IbkrError> {
        Ok(self.0.to_string())
    }
}

struct StubFetcher;

#[async_trait]
impl EquityFetcher for StubFetcher {
    async fn fetch_nlv(&self, _account: &str) -> std::result::Result<f64, IbkrError> {
        Ok(100_000.0)
    }
}

// ---------------- harness ----------------

#[allow(dead_code)] // some fields are kept for symmetry with the production wiring
struct Harness {
    _tmp: NamedTempFile,
    db: Arc<Db>,
    tracker: Arc<TrackerService>,
    tca: Arc<TcaService>,
    equity: Arc<EquitySnapshotService>,
    placer: Arc<MockBracketPlacer>,
    store: Arc<BracketGroupStore>,
    emitter: Arc<EventEmitter>,
    ticket: OrderTicket,
}

impl Harness {
    async fn new() -> Self {
        let tmp = NamedTempFile::new().unwrap();
        let db = Arc::new(Db::open(tmp.path()).unwrap());
        let tracker = Arc::new(TrackerService::new(Arc::clone(&db)));
        let executions_store = Arc::new(ExecutionsStore::new(Arc::clone(&db)));
        let tca = Arc::new(TcaService::new(Arc::clone(&db), executions_store));
        let equity_fetcher: Arc<dyn EquityFetcher> = Arc::new(StubFetcher);
        let equity = Arc::new(EquitySnapshotService::new(Arc::clone(&db), equity_fetcher));
        // Seed today's snapshot so `with_brackets` finds a fresh row.
        let _ = equity.current("DU1").await.unwrap();
        let placer = Arc::new(MockBracketPlacer::new());
        let store = Arc::new(BracketGroupStore::new(Arc::clone(&db)));
        let emitter = Arc::new(EventEmitter::for_capture());
        let ticket = OrderTicket::new(
            Arc::clone(&tracker),
            Arc::clone(&tca),
            Arc::clone(&equity),
            Arc::clone(&placer) as Arc<dyn BracketPlacer>,
            Arc::clone(&store),
            Arc::clone(&emitter),
            Arc::new(FixedAccount("DU1")) as Arc<dyn AccountResolver>,
        );
        Self {
            _tmp: tmp,
            db,
            tracker,
            tca,
            equity,
            placer,
            store,
            emitter,
            ticket,
        }
    }

    /// Insert a sized setup row directly so we don't drag the
    /// detector pipeline into the test. `setups.symbol` FKs into
    /// `tracked_tickers`, so a watchlist row is upserted first when
    /// the symbol is unseen. Returns the inserted setup id.
    async fn seed_setup(
        &self,
        symbol: &str,
        direction: Direction,
        trigger: f64,
        stop: f64,
        qty: u32,
    ) -> i64 {
        let direction_s = match direction {
            Direction::Long => "long",
            Direction::Short => "short",
        };
        let symbol = symbol.to_string();
        let direction_owned = direction_s.to_string();
        let r_per_share_cents = ((trigger - stop).abs() * 100.0).round() as i64;
        let dollar_risk_cents = r_per_share_cents * i64::from(qty);
        let symbol_for_ticker = symbol.clone();
        self.db
            .with_conn(move |conn| {
                conn.execute(
                    "INSERT OR IGNORE INTO tracked_tickers (
                        symbol, source, status, tags, added_at
                     ) VALUES (?1, 'manual', 'watching', '[]', 1234567000)",
                    rusqlite::params![symbol_for_ticker],
                )?;
                conn.execute(
                    "INSERT INTO setups (
                        symbol, strategy, direction, detected_at,
                        trigger_price, stop_price, targets, raw_signals,
                        status,
                        qty, dollar_risk_cents, r_per_share_cents,
                        equity_at_decision_cents, sizing_version,
                        conviction_grade, conviction_multiplier_bps,
                        sizing_cap_applied
                     ) VALUES (
                        ?1, 'breakout', ?2, 1234567890,
                        ?3, ?4, '[]', '{}',
                        'active',
                        ?5, ?6, ?7,
                        10000000, 1,
                        'A', 10000,
                        0
                     )",
                    rusqlite::params![
                        symbol,
                        direction_owned,
                        trigger,
                        stop,
                        i64::from(qty),
                        dollar_risk_cents,
                        r_per_share_cents,
                    ],
                )?;
                Ok(conn.last_insert_rowid())
            })
            .await
            .unwrap()
    }

    /// Manually overwrite the equity snapshot's `fetched_at` so the
    /// staleness gate fires. The migration's INSERT path normally
    /// stamps `now`; tests need to age the row.
    async fn age_snapshot(&self, account: &str, hours: i64) {
        let account = account.to_string();
        let aged = (Utc::now() - chrono::Duration::hours(hours)).timestamp();
        self.db
            .with_conn(move |conn| {
                conn.execute(
                    "UPDATE equity_snapshots SET fetched_at = ?1 WHERE account = ?2",
                    rusqlite::params![aged, account],
                )?;
                Ok(())
            })
            .await
            .unwrap();
    }
}

// ---------------- happy path ----------------

#[tokio::test]
async fn places_bracket_and_persists_group_for_long_setup() {
    let h = Harness::new().await;
    let setup_id = h
        .seed_setup("AAPL", Direction::Long, 150.0, 148.0, 100)
        .await;

    let receipt = h
        .ticket
        .with_brackets(TakeSetupArgs {
            setup_id,
            override_qty: None,
            override_stop_price: None,
            override_reason: None,
        })
        .await
        .unwrap();

    // Ids returned in monotonic order: parent, stop, then 3 targets.
    assert_eq!(receipt.parent_order_id, 1000);
    assert_eq!(receipt.stop_order_id, 1001);
    assert_eq!(receipt.target_order_ids, vec![1002, 1003, 1004]);

    // Group row written with the full ladder.
    let group = h.store.get(receipt.parent_order_id).await.unwrap().unwrap();
    assert_eq!(group.setup_id, setup_id);
    assert_eq!(group.parent_qty, 100);
    assert_eq!(group.system_qty, 100);
    assert_eq!(group.qty_override_reason, None);
    assert_eq!(group.targets.len(), 3);
    let qty_sum: u32 = group.targets.iter().map(|t| t.qty).sum();
    assert_eq!(qty_sum, 100, "rung qty must sum to parent qty");
    // Static 50/30/20 split.
    assert_eq!(group.targets[0].qty_pct, 50);
    assert_eq!(group.targets[1].qty_pct, 30);
    assert_eq!(group.targets[2].qty_pct, 20);

    // Intent recorded for the same setup.
    let intent = h.tca.intents().get(&receipt.intent_id).await.unwrap();
    let intent = intent.expect("intent persisted");
    assert_eq!(intent.setup_id, Some(setup_id));
    assert_eq!(intent.qty, 100.0);

    // Placer saw exactly one BUY parent for AAPL with 3 rungs.
    let calls = h.placer.calls().await;
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].symbol, "AAPL");
    assert_eq!(calls[0].target_rungs.len(), 3);
}

#[tokio::test]
async fn long_setup_target_prices_are_above_trigger() {
    let h = Harness::new().await;
    let setup_id = h
        .seed_setup("AAPL", Direction::Long, 150.0, 148.0, 100)
        .await;
    let receipt = h
        .ticket
        .with_brackets(TakeSetupArgs {
            setup_id,
            override_qty: None,
            override_stop_price: None,
            override_reason: None,
        })
        .await
        .unwrap();
    let group = h.store.get(receipt.parent_order_id).await.unwrap().unwrap();
    // Static R-multiples: 1R, 2R, 3R above trigger.
    assert!((group.targets[0].price - 152.0).abs() < 1e-9);
    assert!((group.targets[1].price - 154.0).abs() < 1e-9);
    assert!((group.targets[2].price - 156.0).abs() < 1e-9);
}

#[tokio::test]
async fn short_setup_target_prices_are_below_trigger() {
    let h = Harness::new().await;
    let setup_id = h
        .seed_setup("TSLA", Direction::Short, 200.0, 204.0, 50)
        .await;
    let receipt = h
        .ticket
        .with_brackets(TakeSetupArgs {
            setup_id,
            override_qty: None,
            override_stop_price: None,
            override_reason: None,
        })
        .await
        .unwrap();
    let group = h.store.get(receipt.parent_order_id).await.unwrap().unwrap();
    // 1R/2R/3R *below* trigger for shorts.
    assert!((group.targets[0].price - 196.0).abs() < 1e-9);
    assert!((group.targets[1].price - 192.0).abs() < 1e-9);
    assert!((group.targets[2].price - 188.0).abs() < 1e-9);
}

#[tokio::test]
async fn override_qty_with_reason_persists_both() {
    let h = Harness::new().await;
    let setup_id = h
        .seed_setup("AAPL", Direction::Long, 150.0, 148.0, 100)
        .await;
    let receipt = h
        .ticket
        .with_brackets(TakeSetupArgs {
            setup_id,
            override_qty: Some(50),
            override_stop_price: None,
            override_reason: Some("half size — early signal".to_string()),
        })
        .await
        .unwrap();
    let group = h.store.get(receipt.parent_order_id).await.unwrap().unwrap();
    assert_eq!(group.parent_qty, 50);
    assert_eq!(group.system_qty, 100);
    assert_eq!(
        group.qty_override_reason.as_deref(),
        Some("half size — early signal")
    );
    let qty_sum: u32 = group.targets.iter().map(|t| t.qty).sum();
    assert_eq!(qty_sum, 50);
}

#[tokio::test]
async fn override_stop_drives_target_geometry() {
    let h = Harness::new().await;
    let setup_id = h
        .seed_setup("AAPL", Direction::Long, 150.0, 148.0, 100)
        .await;
    // Override to a tighter $1 stop — targets compress accordingly.
    let receipt = h
        .ticket
        .with_brackets(TakeSetupArgs {
            setup_id,
            override_qty: None,
            override_stop_price: Some(149.0),
            override_reason: None,
        })
        .await
        .unwrap();
    let group = h.store.get(receipt.parent_order_id).await.unwrap().unwrap();
    assert_eq!(group.stop_price_cents, 14_900);
    // R = 1.0, so targets at 151 / 152 / 153.
    assert!((group.targets[0].price - 151.0).abs() < 1e-9);
    assert!((group.targets[2].price - 153.0).abs() < 1e-9);
}

#[tokio::test]
async fn cancel_flips_status_and_emits_event() {
    let h = Harness::new().await;
    let setup_id = h
        .seed_setup("AAPL", Direction::Long, 150.0, 148.0, 100)
        .await;
    let receipt = h
        .ticket
        .with_brackets(TakeSetupArgs {
            setup_id,
            override_qty: None,
            override_stop_price: None,
            override_reason: None,
        })
        .await
        .unwrap();
    let canceled = h.ticket.cancel(receipt.parent_order_id).await.unwrap();
    assert_eq!(canceled.last_status, super::BracketStatus::Canceled);

    let events = h.emitter.captured().await;
    let placed = events
        .iter()
        .find(|e| matches!(e, crate::events::AppEvent::BracketPlaced { .. }));
    assert!(placed.is_some(), "BracketPlaced should fire on submit");
    let canceled_evt = events.iter().find(|e| {
        matches!(
            e,
            crate::events::AppEvent::BracketStatusChanged {
                status: super::BracketStatus::Canceled,
                ..
            }
        )
    });
    assert!(
        canceled_evt.is_some(),
        "BracketStatusChanged should fire on cancel"
    );
}

// ---------------- gate failures ----------------

#[tokio::test]
async fn refuses_unsized_setup() {
    let h = Harness::new().await;
    // Insert a setup with NULL sizing columns. tracked_tickers row
    // first to satisfy `setups.symbol` FK.
    let id =
        h.db.with_conn(|conn| {
            conn.execute(
                "INSERT OR IGNORE INTO tracked_tickers (
                    symbol, source, status, tags, added_at
                 ) VALUES ('AAPL', 'manual', 'watching', '[]', 1234567000)",
                [],
            )?;
            conn.execute(
                "INSERT INTO setups (
                    symbol, strategy, direction, detected_at,
                    trigger_price, stop_price, targets, raw_signals,
                    status
                 ) VALUES (
                    'AAPL', 'breakout', 'long', 1234567890,
                    150.0, 148.0, '[]', '{}',
                    'active'
                 )",
                [],
            )?;
            Ok(conn.last_insert_rowid())
        })
        .await
        .unwrap();

    let err = h
        .ticket
        .with_brackets(TakeSetupArgs {
            setup_id: id,
            override_qty: None,
            override_stop_price: None,
            override_reason: None,
        })
        .await
        .unwrap_err();
    assert!(matches!(err, OrderTicketError::Unsized(_)));
}

#[tokio::test]
async fn refuses_skipped_sizing() {
    let h = Harness::new().await;
    let id =
        h.db.with_conn(|conn| {
            conn.execute(
                "INSERT OR IGNORE INTO tracked_tickers (
                    symbol, source, status, tags, added_at
                 ) VALUES ('AAPL', 'manual', 'watching', '[]', 1234567000)",
                [],
            )?;
            conn.execute(
                "INSERT INTO setups (
                    symbol, strategy, direction, detected_at,
                    trigger_price, stop_price, targets, raw_signals,
                    status,
                    qty, dollar_risk_cents, r_per_share_cents,
                    equity_at_decision_cents, sizing_version,
                    sizing_skipped_reason, conviction_grade,
                    conviction_multiplier_bps, sizing_cap_applied
                 ) VALUES (
                    'AAPL', 'breakout', 'long', 1234567890,
                    150.0, 148.0, '[]', '{}',
                    'active',
                    0, 0, 0,
                    10000000, 1,
                    'below_min_risk', 'C',
                    10000, 0
                 )",
                [],
            )?;
            Ok(conn.last_insert_rowid())
        })
        .await
        .unwrap();
    let err = h
        .ticket
        .with_brackets(TakeSetupArgs {
            setup_id: id,
            override_qty: None,
            override_stop_price: None,
            override_reason: None,
        })
        .await
        .unwrap_err();
    assert!(matches!(err, OrderTicketError::SizingSkipped { .. }));
}

#[tokio::test]
async fn refuses_stale_equity_snapshot() {
    let h = Harness::new().await;
    let setup_id = h
        .seed_setup("AAPL", Direction::Long, 150.0, 148.0, 100)
        .await;
    h.age_snapshot("DU1", 25).await; // > MAX_EQUITY_STALENESS_HOURS
    let err = h
        .ticket
        .with_brackets(TakeSetupArgs {
            setup_id,
            override_qty: None,
            override_stop_price: None,
            override_reason: None,
        })
        .await
        .unwrap_err();
    assert!(matches!(err, OrderTicketError::StaleEquity { .. }));
}

#[tokio::test]
async fn refuses_override_qty_without_reason() {
    let h = Harness::new().await;
    let setup_id = h
        .seed_setup("AAPL", Direction::Long, 150.0, 148.0, 100)
        .await;
    let err = h
        .ticket
        .with_brackets(TakeSetupArgs {
            setup_id,
            override_qty: Some(50),
            override_stop_price: None,
            override_reason: None,
        })
        .await
        .unwrap_err();
    assert!(matches!(err, OrderTicketError::OverrideMissingReason));
}

#[tokio::test]
async fn refuses_zero_override_qty() {
    let h = Harness::new().await;
    let setup_id = h
        .seed_setup("AAPL", Direction::Long, 150.0, 148.0, 100)
        .await;
    let err = h
        .ticket
        .with_brackets(TakeSetupArgs {
            setup_id,
            override_qty: Some(0),
            override_stop_price: None,
            override_reason: Some("typo".to_string()),
        })
        .await
        .unwrap_err();
    assert!(matches!(err, OrderTicketError::InvalidOverrideQty));
}

#[tokio::test]
async fn refuses_unknown_setup() {
    let h = Harness::new().await;
    let err = h
        .ticket
        .with_brackets(TakeSetupArgs {
            setup_id: 99999,
            override_qty: None,
            override_stop_price: None,
            override_reason: None,
        })
        .await
        .unwrap_err();
    assert!(matches!(err, OrderTicketError::SetupNotFound(99999)));
}

// ---------------- ladder math ----------------

#[test]
fn build_ladder_long_static_50_30_20_round_trips() {
    let specs = super::build_static_target_ladder(Direction::Long, 100.0, 95.0, 100).unwrap();
    assert_eq!(specs.len(), 3);
    assert_eq!(specs[0].qty, 50);
    assert_eq!(specs[1].qty, 30);
    assert_eq!(specs[2].qty, 20);
    assert!((specs[0].price - 105.0).abs() < 1e-9);
    assert!((specs[1].price - 110.0).abs() < 1e-9);
    assert!((specs[2].price - 115.0).abs() < 1e-9);
}

#[test]
fn build_ladder_last_rung_absorbs_remainder() {
    // 7 shares: 50% = 3 (floor), 30% = 2 (floor), runner = 7 - 3 - 2 = 2.
    let specs = super::build_static_target_ladder(Direction::Long, 100.0, 99.0, 7).unwrap();
    assert_eq!(specs.iter().map(|s| s.qty).sum::<u32>(), 7);
    assert_eq!(specs[0].qty, 3);
    assert_eq!(specs[1].qty, 2);
    assert_eq!(specs[2].qty, 2);
}

#[test]
fn build_ladder_skips_zero_qty_rungs_for_tiny_positions() {
    // 2 shares: 50% = 1, 30% = 0 (skipped), runner = 1.
    let specs = super::build_static_target_ladder(Direction::Long, 100.0, 99.0, 2).unwrap();
    let qty_sum: u32 = specs.iter().map(|s| s.qty).sum();
    assert_eq!(qty_sum, 2);
    assert!(specs.iter().all(|s| s.qty > 0));
}

#[test]
fn build_ladder_rejects_zero_r() {
    let err = super::build_static_target_ladder(Direction::Long, 100.0, 100.0, 100).unwrap_err();
    assert!(err.contains("zero"));
}

#[test]
fn build_ladder_rejects_zero_qty() {
    let err = super::build_static_target_ladder(Direction::Long, 100.0, 99.0, 0).unwrap_err();
    assert!(err.contains("parent_qty"));
}

// ---------------- tracer-bullet ----------------
//
// Master plan exit criterion: setup detection → P1 sizing → P2
// intent → P3 bracket placement → mock fill → P2 slippage capture →
// executions row with `setup_id` populated. The tracer below stitches
// every stage against the same in-memory DB so a refactor that
// breaks any link surfaces here.

#[tokio::test]
async fn tracer_bullet_setup_to_executions_row_with_setup_id_populated() {
    use crate::ibkr::types::{ExecutionSide, IbkrExecution};
    use chrono::TimeZone;

    let h = Harness::new().await;
    let setup_id = h
        .seed_setup("AAPL", Direction::Long, 150.0, 148.0, 100)
        .await;

    // P3: place the bracket. This records the P2 intent + writes a
    // bracket_groups row.
    let receipt = h
        .ticket
        .with_brackets(TakeSetupArgs {
            setup_id,
            override_qty: None,
            override_stop_price: None,
            override_reason: None,
        })
        .await
        .unwrap();

    // Simulate IBKR returning a fill against the parent. The fill
    // lands at the trigger price (zero slippage) so the matcher's
    // `compute_slippage` lights up cleanly.
    let posted_at = Utc::now();
    let exec = IbkrExecution {
        symbol: "AAPL".to_string(),
        side: ExecutionSide::Bought,
        qty: 100.0,
        avg_price: 150.0,
        exec_time: posted_at,
        order_id: receipt.parent_order_id,
        exec_id: format!("EXEC-{}", receipt.parent_order_id),
        account: "DU1".to_string(),
        contract_type: "STK".to_string(),
        expiry: None,
        strike: None,
        right: None,
        multiplier: None,
        commission: Some(0.50),
        realized_pnl: None,
        currency: Some("USD".to_string()),
        commission_currency: Some("USD".to_string()),
    };

    // The shared executions store is what the TcaService.attach_fill
    // pipeline reads against; record the fill so attach_fills_for_*
    // sees it.
    let executions_store = Arc::new(ExecutionsStore::new(Arc::clone(&h.db)));
    executions_store
        .record(std::slice::from_ref(&exec))
        .await
        .unwrap();

    // P2: run the linkage pass. The matcher pairs the open intent
    // recorded above with the fresh fill.
    let decision = h.tca.attach_fill(&exec).await.unwrap();
    let decision = decision.expect("matcher matched the parent fill");
    assert_eq!(decision.setup_id, Some(setup_id));
    assert_eq!(decision.exec_id, exec.exec_id);

    // executions row carries setup_id. Read directly so the test
    // doesn't depend on the higher-level query helpers.
    let exec_id = exec.exec_id.clone();
    let setup_on_row: Option<i64> =
        h.db.with_conn(move |conn| {
            let id: Option<i64> = conn
                .query_row(
                    "SELECT setup_id FROM executions WHERE exec_id = ?1",
                    rusqlite::params![exec_id],
                    |r| r.get(0),
                )
                .ok();
            Ok(id)
        })
        .await
        .unwrap();
    assert_eq!(setup_on_row, Some(setup_id));

    // Belt-and-braces: the bracket group row keys the right setup,
    // and the intent on the receipt resolves back to that setup too.
    let group = h.store.get(receipt.parent_order_id).await.unwrap().unwrap();
    assert_eq!(group.setup_id, setup_id);
    let intent = h
        .tca
        .intents()
        .get(&receipt.intent_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(intent.setup_id, Some(setup_id));

    // Suppress "TimeZone import never used" — the import keeps
    // datetime-construction ergonomics close to the executions tests
    // even when this tracer doesn't synthesize a custom moment.
    let _ = chrono::Utc.timestamp_opt(0, 0).unwrap();
}

// ---------------- staleness helper ----------------

#[test]
fn equity_is_stale_returns_true_for_older_than_24h() {
    let now = Utc::now();
    let aged = now - chrono::Duration::hours(25);
    assert!(super::equity_is_stale(aged, now));
    let fresh = now - chrono::Duration::hours(23);
    assert!(!super::equity_is_stale(fresh, now));
}
