//! Phase 7 — `BracketReviser` integration tests.
//!
//! Drive the math + trait-seam plumbing without spinning up an IBKR
//! connection: a mock `BracketModifier` records calls, a mock
//! `QuoteSource` returns canned price observations, and an in-memory
//! SQLite database hosts the bracket_groups + setups rows. The tests
//! pin the master decisions: trail activates after the 1×ATR rung is
//! reached, BE-move fires at 1R, the chandelier stop never moves
//! against the trader, and pre-P7 brackets (NULL exit_plan_json) are
//! left alone.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use tempfile::NamedTempFile;
use tokio::sync::Mutex;

use crate::ibkr::error::IbkrError;
use crate::ibkr::types::ModifyStopRequest;
use crate::services::order_ticket::{
    BracketGroupRecord, BracketGroupStore, BracketModifier, BracketStatus, TargetSpec,
};
use crate::services::tracker_service::TrackerService;
use crate::storage::Db;
use crate::strategies::exits::{AtrScaled, ChandelierState, ExitPolicy, ExitPolicyContext};
use crate::strategies::Direction;

use super::{BracketReviser, PriceObservation, QuoteSource, ReviseDecision};

#[derive(Clone, Default)]
struct MockModifier {
    inbox: Arc<Mutex<Vec<ModifyStopRequest>>>,
}

#[async_trait]
impl BracketModifier for MockModifier {
    async fn modify_stop(&self, req: ModifyStopRequest) -> std::result::Result<(), IbkrError> {
        self.inbox.lock().await.push(req);
        Ok(())
    }
}

impl MockModifier {
    fn new() -> Self {
        Self {
            inbox: Arc::new(Mutex::new(Vec::new())),
        }
    }
    async fn calls(&self) -> Vec<ModifyStopRequest> {
        self.inbox.lock().await.clone()
    }
}

#[derive(Clone)]
struct CannedQuote {
    extreme_price: f64,
    current_price: f64,
}

#[async_trait]
impl QuoteSource for CannedQuote {
    async fn observe(&self, _symbol: &str) -> std::result::Result<PriceObservation, IbkrError> {
        Ok(PriceObservation {
            extreme_price: self.extreme_price,
            current_price: self.current_price,
        })
    }
}

struct Harness {
    _tmp: NamedTempFile,
    db: Arc<Db>,
    tracker: Arc<TrackerService>,
    bracket_store: Arc<BracketGroupStore>,
}

impl Harness {
    async fn new() -> Self {
        let tmp = NamedTempFile::new().unwrap();
        let db = Arc::new(Db::open(tmp.path()).unwrap());
        let tracker = Arc::new(TrackerService::new(Arc::clone(&db)));
        let bracket_store = Arc::new(BracketGroupStore::new(Arc::clone(&db)));
        // Seed FK target rows directly. Mirrors order_ticket/store
        // tests; the bracket-reviser's flow expects an existing
        // setups row + intent row so its FKs to bracket_groups don't
        // panic.
        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO tracked_tickers (
                    symbol, source, status, tags, added_at
                 ) VALUES ('AAPL', 'manual', 'watching', '[]', 1234567000)",
                [],
            )?;
            conn.execute(
                "INSERT INTO setups (
                    id, symbol, strategy, direction, detected_at,
                    trigger_price, stop_price, targets, raw_signals,
                    status
                 ) VALUES (
                    42, 'AAPL', 'breakout', 'long', 1234567890,
                    100.0, 98.0, '[]', '{}',
                    'active'
                 )",
                [],
            )?;
            conn.execute(
                "INSERT INTO order_intents (
                    intent_id, setup_id, account, symbol, side, qty,
                    intended_price_cents, intended_price_source,
                    posted_at, expires_at
                 ) VALUES (
                    'intent_s42_xyz', 42, 'DU1', 'AAPL', 'buy', 100,
                    10000, 'trigger_price',
                    '2026-05-05T13:00:00Z', '2026-05-05T14:00:00Z'
                 )",
                [],
            )?;
            Ok(())
        })
        .await
        .unwrap();
        Self {
            _tmp: tmp,
            db,
            tracker,
            bracket_store,
        }
    }

    async fn seed_v2_plan(&self, atr: f64) {
        let plan = AtrScaled::new(10)
            .build_plan(&ExitPolicyContext {
                direction: Direction::Long,
                trigger_price: 100.0,
                stop_price: 98.0,
                atr: Some(atr),
                strategy: "breakout",
            })
            .unwrap();
        let plan_json = serde_json::to_value(&plan).unwrap();
        self.tracker
            .update_setup_exit_plan(42, plan.policy_version.clone(), plan_json)
            .await
            .unwrap();
    }

    async fn seed_bracket(&self, atr: f64) {
        // Match the v2 plan's first rung at 1×ATR above entry.
        let target_price = 100.0 + atr;
        let record = BracketGroupRecord {
            parent_order_id: 1001,
            setup_id: 42,
            intent_id: "intent_s42_xyz".to_string(),
            account: "DU1".to_string(),
            symbol: "AAPL".to_string(),
            direction: "long".to_string(),
            parent_qty: 100,
            system_qty: 100,
            qty_override_reason: None,
            entry_limit_cents: 10_000,
            stop_order_id: 1002,
            stop_price_cents: 9_800,
            target_order_ids: vec![1003, 1004, 1005],
            targets: vec![TargetSpec {
                label: "1xATR".to_string(),
                price: target_price,
                qty: 50,
                qty_pct: 50,
            }],
            placed_at: Utc::now(),
            last_status: BracketStatus::Open,
            last_status_at: Utc::now(),
        };
        self.bracket_store.insert(record).await.unwrap();
    }
}

#[tokio::test]
async fn revise_skips_when_no_plan_persisted() {
    let h = Harness::new().await;
    h.seed_bracket(1.5).await;
    let modifier = Arc::new(MockModifier::new());
    let reviser = BracketReviser::new(
        Arc::clone(&h.bracket_store),
        Arc::clone(&h.tracker),
        modifier.clone(),
        Arc::new(CannedQuote {
            extreme_price: 110.0,
            current_price: 110.0,
        }),
    );
    let bracket = h.bracket_store.get(1001).await.unwrap().unwrap();
    let decision = reviser.revise_one_bracket(&bracket).await.unwrap();
    assert_eq!(decision, ReviseDecision::Skipped);
    assert!(modifier.calls().await.is_empty());
}

#[tokio::test]
async fn revise_pre_activation_does_not_modify_stop() {
    let h = Harness::new().await;
    h.seed_v2_plan(1.5).await;
    h.seed_bracket(1.5).await;
    let modifier = Arc::new(MockModifier::new());
    // Price 100.5: still below the 1×ATR rung (101.5) → trail not
    // activated. Should not modify.
    let reviser = BracketReviser::new(
        Arc::clone(&h.bracket_store),
        Arc::clone(&h.tracker),
        modifier.clone(),
        Arc::new(CannedQuote {
            extreme_price: 100.5,
            current_price: 100.5,
        }),
    );
    let bracket = h.bracket_store.get(1001).await.unwrap().unwrap();
    let decision = reviser.revise_one_bracket(&bracket).await.unwrap();
    assert_eq!(decision, ReviseDecision::NoChange);
    assert!(modifier.calls().await.is_empty());
}

#[tokio::test]
async fn revise_activates_and_raises_stop_after_1x_atr_rung_reached() {
    let h = Harness::new().await;
    let atr = 1.5;
    h.seed_v2_plan(atr).await;
    h.seed_bracket(atr).await;
    let modifier = Arc::new(MockModifier::new());
    // Extreme = 105 → trail = 105 - 3*1.5 = 100.5 (above 98 stop).
    // Current = 102 → past 1×ATR rung (101.5).
    let reviser = BracketReviser::new(
        Arc::clone(&h.bracket_store),
        Arc::clone(&h.tracker),
        modifier.clone(),
        Arc::new(CannedQuote {
            extreme_price: 105.0,
            current_price: 102.0,
        }),
    );
    let bracket = h.bracket_store.get(1001).await.unwrap().unwrap();
    let decision = reviser.revise_one_bracket(&bracket).await.unwrap();
    match decision {
        ReviseDecision::StopRaised { old_stop, new_stop } => {
            // BE move fires at 1R = 102 first, raising stop to 100;
            // chandelier (100.5) wins over BE (100). Final new_stop
            // = 100.5.
            assert!((old_stop - 100.0).abs() < 1e-2);
            assert!((new_stop - 100.5).abs() < 1e-2);
        }
        other => panic!("expected StopRaised, got {other:?}"),
    }
    let calls = modifier.calls().await;
    assert_eq!(calls.len(), 1);
    assert!((calls[0].new_stop_price - 100.5).abs() < 1e-2);
    assert_eq!(calls[0].stop_order_id, 1002);
    assert_eq!(calls[0].oca_group, "br-1001");
    // State persisted.
    let st = h
        .bracket_store
        .get_trail_state(1001)
        .await
        .unwrap()
        .unwrap();
    assert!(st.activated);
    assert!(st.be_moved);
}

#[tokio::test]
async fn revise_does_not_lower_stop_on_drawdown() {
    let h = Harness::new().await;
    h.seed_v2_plan(1.5).await;
    h.seed_bracket(1.5).await;
    let modifier = Arc::new(MockModifier::new());
    let reviser = BracketReviser::new(
        Arc::clone(&h.bracket_store),
        Arc::clone(&h.tracker),
        modifier.clone(),
        Arc::new(CannedQuote {
            extreme_price: 105.0,
            current_price: 102.0,
        }),
    );
    let bracket = h.bracket_store.get(1001).await.unwrap().unwrap();
    let _ = reviser.revise_one_bracket(&bracket).await.unwrap();
    let _ = reviser.revise_one_bracket(&bracket).await.unwrap();
    let calls_before = modifier.calls().await.len();

    // Switch to a worse observation: drawdown → trail must not
    // move down. The chandelier never moves against the trader.
    let reviser_dd = BracketReviser::new(
        Arc::clone(&h.bracket_store),
        Arc::clone(&h.tracker),
        modifier.clone(),
        Arc::new(CannedQuote {
            extreme_price: 102.5,
            current_price: 101.5,
        }),
    );
    let bracket = h.bracket_store.get(1001).await.unwrap().unwrap();
    let decision = reviser_dd.revise_one_bracket(&bracket).await.unwrap();
    assert_eq!(decision, ReviseDecision::NoChange);
    assert_eq!(modifier.calls().await.len(), calls_before);
}

#[tokio::test]
async fn revise_aborts_modify_when_status_changed_mid_poll() {
    let h = Harness::new().await;
    h.seed_v2_plan(1.5).await;
    h.seed_bracket(1.5).await;
    // Pre-flip the bracket to Canceled so the reviser's status
    // re-check refuses to modify.
    h.bracket_store
        .update_status(1001, BracketStatus::Canceled, Utc::now())
        .await
        .unwrap();
    let modifier = Arc::new(MockModifier::new());
    let reviser = BracketReviser::new(
        Arc::clone(&h.bracket_store),
        Arc::clone(&h.tracker),
        modifier.clone(),
        Arc::new(CannedQuote {
            extreme_price: 110.0,
            current_price: 102.0,
        }),
    );
    // Have to construct the bracket from the still-open shape to
    // trigger the path; sweep listing won't include canceled rows.
    // Build from scratch with last_status=Open so the per-bracket
    // call runs and the inner re-check trips.
    let mut bracket = h.bracket_store.get(1001).await.unwrap().unwrap();
    bracket.last_status = BracketStatus::Open;
    let decision = reviser.revise_one_bracket(&bracket).await.unwrap();
    // The inner re-check returns NoChange.
    assert_eq!(decision, ReviseDecision::NoChange);
    assert!(modifier.calls().await.is_empty());
}

#[tokio::test]
async fn run_sweep_skips_canceled_brackets() {
    let h = Harness::new().await;
    h.seed_v2_plan(1.5).await;
    h.seed_bracket(1.5).await;
    h.bracket_store
        .update_status(1001, BracketStatus::Canceled, Utc::now())
        .await
        .unwrap();
    let modifier = Arc::new(MockModifier::new());
    let reviser = BracketReviser::new(
        Arc::clone(&h.bracket_store),
        Arc::clone(&h.tracker),
        modifier.clone(),
        Arc::new(CannedQuote {
            extreme_price: 105.0,
            current_price: 102.0,
        }),
    );
    let decisions = reviser.run_sweep().await.unwrap();
    assert!(decisions.is_empty());
    assert!(modifier.calls().await.is_empty());
}

#[tokio::test]
async fn snapshot_returns_active_brackets_with_remaining_days() {
    let h = Harness::new().await;
    h.seed_v2_plan(1.5).await;
    h.seed_bracket(1.5).await;
    let modifier = Arc::new(MockModifier::new());
    let reviser = BracketReviser::new(
        Arc::clone(&h.bracket_store),
        Arc::clone(&h.tracker),
        modifier,
        Arc::new(CannedQuote {
            extreme_price: 100.0,
            current_price: 100.0,
        }),
    );
    let snaps = reviser.snapshot().await.unwrap();
    assert_eq!(snaps.len(), 1);
    let s = &snaps[0];
    assert_eq!(s.parent_order_id, 1001);
    assert_eq!(s.symbol, "AAPL");
    // breakout maps to 10-day time-stop.
    assert!(s.time_stop_remaining_days.is_some());
    let _ = h.db; // silence unused
}

#[tokio::test]
async fn revise_signals_time_stop_elapsed_after_horizon() {
    let h = Harness::new().await;
    h.seed_v2_plan(1.5).await;
    // Override placed_at into the deep past so the time-stop
    // horizon (10 BD breakout) is well-past.
    let record = BracketGroupRecord {
        parent_order_id: 2001,
        setup_id: 42,
        intent_id: "intent_s42_xyz".to_string(),
        account: "DU1".to_string(),
        symbol: "AAPL".to_string(),
        direction: "long".to_string(),
        parent_qty: 100,
        system_qty: 100,
        qty_override_reason: None,
        entry_limit_cents: 10_000,
        stop_order_id: 2002,
        stop_price_cents: 9_800,
        target_order_ids: vec![],
        targets: vec![],
        placed_at: Utc::now() - chrono::Duration::days(60),
        last_status: BracketStatus::Open,
        last_status_at: Utc::now(),
    };
    h.bracket_store.insert(record).await.unwrap();
    let modifier = Arc::new(MockModifier::new());
    let reviser = BracketReviser::new(
        Arc::clone(&h.bracket_store),
        Arc::clone(&h.tracker),
        modifier.clone(),
        Arc::new(CannedQuote {
            extreme_price: 100.0,
            current_price: 100.0,
        }),
    );
    let bracket = h.bracket_store.get(2001).await.unwrap().unwrap();
    let decision = reviser.revise_one_bracket(&bracket).await.unwrap();
    assert_eq!(decision, ReviseDecision::TimeStopElapsed);
    // No modify call — time-stop is operator-driven for now.
    assert!(modifier.calls().await.is_empty());
}

#[tokio::test]
async fn revise_persists_extreme_state_even_when_no_modify() {
    let h = Harness::new().await;
    let _state_before: Option<ChandelierState> =
        h.bracket_store.get_trail_state(1001).await.unwrap();
    h.seed_v2_plan(1.5).await;
    h.seed_bracket(1.5).await;
    let modifier = Arc::new(MockModifier::new());
    let reviser = BracketReviser::new(
        Arc::clone(&h.bracket_store),
        Arc::clone(&h.tracker),
        modifier.clone(),
        Arc::new(CannedQuote {
            extreme_price: 100.5,
            current_price: 100.5,
        }),
    );
    let bracket = h.bracket_store.get(1001).await.unwrap().unwrap();
    let decision = reviser.revise_one_bracket(&bracket).await.unwrap();
    assert_eq!(decision, ReviseDecision::NoChange);
    // State seeded — extreme tracks the observation.
    let st = h
        .bracket_store
        .get_trail_state(1001)
        .await
        .unwrap()
        .unwrap();
    assert!((st.extreme_price - 100.5).abs() < 1e-9);
    assert!(!st.activated);
    assert!(modifier.calls().await.is_empty());
}
