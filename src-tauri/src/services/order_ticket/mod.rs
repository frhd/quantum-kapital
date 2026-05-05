//! Phase 3 — `services/order_ticket/`: bracket-on-activation chokepoint.
//!
//! Master Hard Invariant 1 (surveillance + confirmed execution): no
//! scheduler / detector / agent / LLM ever places a parent order. The
//! single Tauri command `order_ticket_take_setup` is the *only* path
//! to live order placement; it pulls Phase 1 sizing, records a Phase 2
//! intent, sends the parent + stop + N targets atomically through
//! `IbkrClient::place_bracket`, and persists a `bracket_groups` row
//! linking everything back to the originating `setup_id`.
//!
//! The static 50/30/20 ladder lives here as a const — master phase
//! decision: "ship with 50/30/20 fixed; ATR-trail logic is P7."

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{Duration as ChronoDuration, Utc};
use thiserror::Error;
use tracing::warn;

use crate::events::{AppEvent, EventEmitter};
use crate::ibkr::client::IbkrClient;
use crate::ibkr::error::IbkrError;
use crate::ibkr::types::{BracketReceipt, BracketRequest, ModifyStopRequest, OrderAction};
use crate::services::risk_engine::{EquitySnapshotError, EquitySnapshotService};
use crate::services::tca::{IntendedPriceSource, IntentSide, NewOrderIntent, TcaError, TcaService};
use crate::services::tracker_service::{TrackerError, TrackerService};
use crate::storage::error::StorageError;
use crate::strategies::exits::ExitPlan;
use crate::strategies::Direction;

mod store;
mod types;

#[cfg(test)]
mod tests;

pub use store::BracketGroupStore;
pub use types::{
    BracketGroupRecord, BracketStatus, TargetSpec, TicketReceipt, MAX_EQUITY_STALENESS_HOURS,
    STATIC_TARGET_LADDER_PCT, STATIC_TARGET_R_MULTIPLES,
};

/// Trait seam over IBKR bracket placement. Production: `IbkrClient`
/// wraps `place_order` with the transmit-flag dance. Tests:
/// `MockBracketPlacer` (in tests.rs) records calls and hands back
/// canned receipts so the mock-based bracket sim doesn't depend on a
/// live IBKR connection.
#[async_trait]
pub trait BracketPlacer: Send + Sync {
    async fn place_bracket(
        &self,
        req: BracketRequest,
    ) -> std::result::Result<BracketReceipt, IbkrError>;
}

#[async_trait]
impl BracketPlacer for IbkrClient {
    async fn place_bracket(
        &self,
        req: BracketRequest,
    ) -> std::result::Result<BracketReceipt, IbkrError> {
        IbkrClient::place_bracket(self, req).await
    }
}

/// Phase 7 — trait seam over IBKR stop-modify. Production: real
/// `IbkrClient` re-submits the stop child via `place_order` with the
/// existing `order_id`. Tests: `MockBracketModifier` records calls
/// and returns canned outcomes so the reviser is exercisable
/// without a live IBKR connection.
#[async_trait]
pub trait BracketModifier: Send + Sync {
    async fn modify_stop(&self, req: ModifyStopRequest) -> std::result::Result<(), IbkrError>;
}

#[async_trait]
impl BracketModifier for IbkrClient {
    async fn modify_stop(&self, req: ModifyStopRequest) -> std::result::Result<(), IbkrError> {
        IbkrClient::modify_stop_price(self, req).await
    }
}

#[derive(Error, Debug)]
pub enum OrderTicketError {
    #[error("setup#{0} not found")]
    SetupNotFound(i64),
    #[error("setup#{0} has no sizing — run risk engine before placing a bracket")]
    Unsized(i64),
    #[error("setup#{setup_id} sizing was skipped: {reason}")]
    SizingSkipped { setup_id: i64, reason: String },
    #[error(
        "equity snapshot is stale ({age_hours}h ≥ {max_hours}h) — refresh equity before sending"
    )]
    StaleEquity { age_hours: i64, max_hours: i64 },
    #[error("override_qty must be > 0 when supplied")]
    InvalidOverrideQty,
    #[error("override_qty supplied without a reason")]
    OverrideMissingReason,
    #[error("invalid setup geometry: {0}")]
    InvalidGeometry(String),
    #[error("ibkr: {0}")]
    Ibkr(#[from] IbkrError),
    #[error("tca: {0}")]
    Tca(#[from] TcaError),
    #[error("tracker: {0}")]
    Tracker(#[from] TrackerError),
    #[error("equity: {0}")]
    Equity(#[from] EquitySnapshotError),
    #[error("storage: {0}")]
    Storage(#[from] StorageError),
}

pub type Result<T> = std::result::Result<T, OrderTicketError>;

/// Args for `OrderTicket::with_brackets`. Mirrors the shape the modal
/// posts: setup id is required, qty + stop overrides are optional but
/// must come paired with a free-text reason.
#[derive(Debug, Clone)]
pub struct TakeSetupArgs {
    pub setup_id: i64,
    pub override_qty: Option<u32>,
    pub override_stop_price: Option<f64>,
    pub override_reason: Option<String>,
}

/// The service. Cheap to clone — internal state is just `Arc`s.
#[derive(Clone)]
pub struct OrderTicket {
    tracker: Arc<TrackerService>,
    tca: Arc<TcaService>,
    equity: Arc<EquitySnapshotService>,
    placer: Arc<dyn BracketPlacer>,
    store: Arc<BracketGroupStore>,
    emitter: Arc<EventEmitter>,
    /// IBKR account to attribute the bracket to. Pulled from
    /// `RiskEngine::AccountSource` upstream and set once at boot.
    account_resolver: Arc<dyn AccountResolver>,
}

/// Trait seam for "which account does this bracket post against?".
/// Mirrors `services::risk_engine::AccountSource` — production wires
/// `IbkrClient` (first account from `get_accounts`); tests hand-roll a
/// fixed-string stub so the bracket flow is exercisable without an
/// IBKR connection.
#[async_trait]
pub trait AccountResolver: Send + Sync {
    async fn account(&self) -> std::result::Result<String, IbkrError>;
}

#[async_trait]
impl AccountResolver for IbkrClient {
    async fn account(&self) -> std::result::Result<String, IbkrError> {
        let accounts = self.get_accounts().await?;
        accounts
            .into_iter()
            .next()
            .ok_or_else(|| IbkrError::RequestFailed("no IBKR accounts available".to_string()))
    }
}

impl OrderTicket {
    pub fn new(
        tracker: Arc<TrackerService>,
        tca: Arc<TcaService>,
        equity: Arc<EquitySnapshotService>,
        placer: Arc<dyn BracketPlacer>,
        store: Arc<BracketGroupStore>,
        emitter: Arc<EventEmitter>,
        account_resolver: Arc<dyn AccountResolver>,
    ) -> Self {
        Self {
            tracker,
            tca,
            equity,
            placer,
            store,
            emitter,
            account_resolver,
        }
    }

    /// Single chokepoint for setup-linked order submission. Pre-flight
    /// gates (master Decisions in this phase): sizing must be present
    /// and not skipped; equity snapshot < 24h; override-qty → require
    /// reason. On success: TCA intent recorded, bracket placed, group
    /// row persisted, `BracketPlaced` emitted.
    pub async fn with_brackets(&self, args: TakeSetupArgs) -> Result<TicketReceipt> {
        let setup = self
            .tracker
            .get_setup(args.setup_id)
            .await?
            .ok_or(OrderTicketError::SetupNotFound(args.setup_id))?;

        let sizing = setup
            .sizing
            .clone()
            .ok_or(OrderTicketError::Unsized(args.setup_id))?;
        if let Some(reason) = sizing.skipped_reason {
            return Err(OrderTicketError::SizingSkipped {
                setup_id: args.setup_id,
                reason: reason.as_str().to_string(),
            });
        }

        let parent_qty = match args.override_qty {
            Some(0) => return Err(OrderTicketError::InvalidOverrideQty),
            Some(q) => q,
            None => sizing.qty,
        };
        if args.override_qty.is_some() && args.override_reason.is_none() {
            return Err(OrderTicketError::OverrideMissingReason);
        }

        let stop_price = args.override_stop_price.unwrap_or(setup.stop_price);

        // Account resolution + equity-snapshot freshness gate. Master
        // decision: hard-block when the snapshot the sizing pinned to
        // is older than 24h; the trader must `risk_refresh_equity`
        // before send.
        let account = self.account_resolver.account().await?;
        let snap = self
            .equity
            .read(&account, &today_et())
            .await?
            .ok_or_else(|| OrderTicketError::StaleEquity {
                age_hours: i64::MAX,
                max_hours: MAX_EQUITY_STALENESS_HOURS,
            })?;
        let age = Utc::now() - snap.fetched_at;
        if age > ChronoDuration::hours(MAX_EQUITY_STALENESS_HOURS) {
            return Err(OrderTicketError::StaleEquity {
                age_hours: age.num_hours(),
                max_hours: MAX_EQUITY_STALENESS_HOURS,
            });
        }

        // Phase 7 — prefer the per-detector exit plan persisted by
        // the runner. Falls back to the static 50/30/20 ladder for
        // pre-P7 rows (NULL plan column) and for runs where the
        // policy refused (e.g. ATR unavailable). The fallback path
        // matches Phase 3's behavior exactly so pre-P7 setups stay
        // shippable under the same wire shape.
        let exit_plan = self
            .tracker
            .get_setup_exit_plan(args.setup_id)
            .await
            .ok()
            .flatten();
        let targets = match exit_plan.as_ref() {
            Some(plan) => target_specs_from_plan(plan, parent_qty)
                .map_err(|m| OrderTicketError::InvalidGeometry(m.to_string()))?,
            None => build_static_target_ladder(
                setup.direction,
                setup.trigger_price,
                stop_price,
                parent_qty,
            )
            .map_err(|m| OrderTicketError::InvalidGeometry(m.to_string()))?,
        };

        // Record the Phase 2 intent before sending. The intent links
        // the future fill back to `setup_id` for attribution; even if
        // the bracket placement fails, the intent stays in the
        // ledger as a paper-trail of the trader's decision (it
        // expires on its own).
        let intent_id = gen_intent_id(args.setup_id);
        let now = Utc::now();
        let intent = NewOrderIntent {
            intent_id: intent_id.clone(),
            setup_id: Some(args.setup_id),
            account: account.clone(),
            symbol: setup.symbol.clone(),
            side: match setup.direction {
                Direction::Long => IntentSide::Buy,
                Direction::Short => IntentSide::Sell,
            },
            qty: f64::from(parent_qty),
            intended_price_cents: (setup.trigger_price * 100.0).round() as i64,
            intended_price_source: IntendedPriceSource::TriggerPrice,
            posted_at: now,
            // Bracket entry is a LIMIT — use the longer 60-min
            // window so a sat-on-the-bid fill still matches if the
            // trader doesn't cancel.
            expires_at: now + ChronoDuration::minutes(60),
        };
        self.tca.record_intent(intent).await?;

        let req = BracketRequest {
            symbol: setup.symbol.clone(),
            entry_action: match setup.direction {
                Direction::Long => OrderAction::Buy,
                Direction::Short => OrderAction::Sell,
            },
            qty: f64::from(parent_qty),
            entry_limit_price: setup.trigger_price,
            stop_price,
            target_rungs: targets
                .iter()
                .map(|t| (t.price, f64::from(t.qty)))
                .collect(),
        };
        let receipt = self.placer.place_bracket(req).await?;

        let record = BracketGroupRecord {
            parent_order_id: receipt.parent_order_id,
            setup_id: args.setup_id,
            intent_id: intent_id.clone(),
            account: account.clone(),
            symbol: setup.symbol.clone(),
            direction: direction_str(setup.direction).to_string(),
            parent_qty,
            system_qty: sizing.qty,
            qty_override_reason: args.override_reason.clone(),
            entry_limit_cents: (setup.trigger_price * 100.0).round() as i64,
            stop_order_id: receipt.stop_order_id,
            stop_price_cents: (stop_price * 100.0).round() as i64,
            target_order_ids: receipt.target_order_ids.clone(),
            targets: targets.clone(),
            placed_at: now,
            last_status: BracketStatus::Open,
            last_status_at: now,
        };
        if let Err(e) = self.store.insert(record.clone()).await {
            warn!(
                parent_order_id = receipt.parent_order_id,
                "order_ticket: bracket placed but store.insert failed: {e}"
            );
            return Err(e.into());
        }

        let _ = self
            .emitter
            .emit(AppEvent::BracketPlaced {
                parent_order_id: receipt.parent_order_id,
                setup_id: args.setup_id,
                symbol: setup.symbol.clone(),
                qty: parent_qty,
            })
            .await;

        Ok(TicketReceipt {
            parent_order_id: receipt.parent_order_id,
            stop_order_id: receipt.stop_order_id,
            target_order_ids: receipt.target_order_ids,
            intent_id,
            setup_id: args.setup_id,
            placed_at: now,
        })
    }

    /// Read the persisted bracket group for `parent_order_id`. Powers
    /// `order_ticket_status`. `None` ↔ no group on file (the parent
    /// id wasn't placed through this service).
    pub async fn status(&self, parent_order_id: i32) -> Result<Option<BracketGroupRecord>> {
        Ok(self.store.get(parent_order_id).await?)
    }

    /// Cancel an open bracket group. Flips `last_status` to
    /// `Canceled` and emits `BracketStatusChanged`. The actual IBKR
    /// cancel is fire-and-forget for now — the post-fill reconciler
    /// (later phase) will reconcile against IBKR's `orderStatus`
    /// stream. Returns the updated record.
    pub async fn cancel(&self, parent_order_id: i32) -> Result<BracketGroupRecord> {
        let now = Utc::now();
        let updated = self
            .store
            .update_status(parent_order_id, BracketStatus::Canceled, now)
            .await?;
        if !updated {
            return Err(OrderTicketError::Storage(StorageError::Migration(format!(
                "bracket_groups#{parent_order_id} not found"
            ))));
        }
        let record = self.store.get(parent_order_id).await?.ok_or_else(|| {
            OrderTicketError::Storage(StorageError::Migration(format!(
                "bracket_groups#{parent_order_id} disappeared after update"
            )))
        })?;
        let _ = self
            .emitter
            .emit(AppEvent::BracketStatusChanged {
                parent_order_id,
                setup_id: record.setup_id,
                status: BracketStatus::Canceled,
            })
            .await;
        Ok(record)
    }
}

/// Compute the static 50/30/20 R-multiple target ladder for a setup.
/// Errors when geometry is invalid (zero R, non-finite prices) — the
/// risk engine already rejects these but the ladder builder is also
/// called from tests, so it stays defensive.
pub(crate) fn build_static_target_ladder(
    direction: Direction,
    trigger: f64,
    stop: f64,
    parent_qty: u32,
) -> std::result::Result<Vec<TargetSpec>, &'static str> {
    if !trigger.is_finite() || !stop.is_finite() {
        return Err("trigger and stop must be finite");
    }
    let r = (trigger - stop).abs();
    if r == 0.0 {
        return Err("trigger and stop are equal — risk distance is zero");
    }
    if parent_qty == 0 {
        return Err("parent_qty must be > 0");
    }
    let signed = match direction {
        Direction::Long => 1.0,
        Direction::Short => -1.0,
    };

    let pcts = STATIC_TARGET_LADDER_PCT;
    let r_multiples = STATIC_TARGET_R_MULTIPLES;
    debug_assert_eq!(pcts.len(), r_multiples.len());

    // Whole-share allocation — earlier rungs round down, the last
    // rung absorbs the remainder so total qty matches `parent_qty`
    // exactly.
    let mut allocated: u32 = 0;
    let mut specs = Vec::with_capacity(pcts.len());
    for (idx, (&pct, &mult)) in pcts.iter().zip(r_multiples.iter()).enumerate() {
        let is_last = idx + 1 == pcts.len();
        let qty = if is_last {
            parent_qty - allocated
        } else {
            // Floor division — never overshoot the parent qty.
            let raw = (u64::from(parent_qty) * u64::from(pct)) / 100;
            let q = raw as u32;
            allocated += q;
            q
        };
        if qty == 0 {
            // Skip zero-qty rungs — happens when parent_qty < 5 and
            // the 20% remainder rounds to zero. The 50/30/20 split
            // is preserved in spirit; the trader sees a 1- or 2-leg
            // ladder for tiny positions instead of a 3-leg one with
            // a phantom zero-qty rung.
            continue;
        }
        specs.push(TargetSpec {
            label: format!("{:.0}R", mult),
            price: trigger + signed * mult * r,
            qty,
            qty_pct: pct,
        });
    }
    Ok(specs)
}

/// Phase 7 — materialize whole-share `TargetSpec` rungs from a frozen
/// `ExitPlan`. Mirrors the qty-allocation rules in
/// `build_static_target_ladder`: floor-divide each rung's pct against
/// `parent_qty`; the last non-zero rung absorbs the rounding
/// remainder so the bracket sums to `parent_qty` exactly. Zero-qty
/// rungs (when parent_qty is too small for the pct to round to a
/// share) are dropped so the bracket sees a tighter ladder rather
/// than a phantom zero-leg.
pub(crate) fn target_specs_from_plan(
    plan: &ExitPlan,
    parent_qty: u32,
) -> std::result::Result<Vec<TargetSpec>, &'static str> {
    if parent_qty == 0 {
        return Err("parent_qty must be > 0");
    }
    if plan.targets.is_empty() {
        return Err("exit plan has no target rungs");
    }
    // Pre-validate prices early so a bad plan fails before partial
    // allocation runs.
    for t in &plan.targets {
        if !t.price.is_finite() {
            return Err("target price not finite");
        }
    }
    let pct_total: u32 = plan.targets.iter().map(|t| t.qty_pct as u32).sum();
    if pct_total != 100 {
        return Err("exit plan target pcts must sum to 100");
    }

    let mut allocated: u32 = 0;
    let mut specs: Vec<TargetSpec> = Vec::with_capacity(plan.targets.len());
    let last_idx = plan.targets.len() - 1;
    for (idx, t) in plan.targets.iter().enumerate() {
        let qty = if idx == last_idx {
            parent_qty.saturating_sub(allocated)
        } else {
            let raw = (u64::from(parent_qty) * u64::from(t.qty_pct)) / 100;
            let q = raw as u32;
            allocated = allocated.saturating_add(q);
            q
        };
        if qty == 0 {
            continue;
        }
        specs.push(TargetSpec {
            label: t.label.clone(),
            price: t.price,
            qty,
            qty_pct: t.qty_pct,
        });
    }
    if specs.is_empty() {
        return Err("exit plan resolved to zero rungs after qty allocation");
    }
    Ok(specs)
}

fn direction_str(direction: Direction) -> &'static str {
    match direction {
        Direction::Long => "long",
        Direction::Short => "short",
    }
}

fn today_et() -> String {
    use chrono_tz::America::New_York;
    Utc::now()
        .with_timezone(&New_York)
        .date_naive()
        .format("%Y-%m-%d")
        .to_string()
}

/// Process-local intent id generator. Mirrors the format used by
/// `ibkr/commands/trading.rs` so attribution queries can `LIKE
/// 'intent_s%'` to find setup-linked intents regardless of which
/// command path generated them.
fn gen_intent_id(setup_id: i64) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("intent_s{setup_id}_{nanos}_{n}")
}

/// Convenience: equity-staleness check used by the modal banner
/// when it has the snapshot timestamp in hand. Mirrors the same gate
/// the service applies inside `with_brackets` so the UI can grey out
/// the Send button before the round-trip rejects.
#[cfg(test)]
pub(crate) fn equity_is_stale(
    snapshot_fetched_at: chrono::DateTime<Utc>,
    now: chrono::DateTime<Utc>,
) -> bool {
    now - snapshot_fetched_at > ChronoDuration::hours(MAX_EQUITY_STALENESS_HOURS)
}
