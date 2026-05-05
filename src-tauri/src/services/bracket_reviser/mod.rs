//! Phase 7 — `BracketReviser`: poll-based stop-modify rail.
//!
//! Every minute during RTH, the reviser looks at every open or
//! partial bracket, fetches a fresh observation (high/low or
//! current quote), updates the chandelier high-water-mark, and
//! decides whether to step the stop child up.
//!
//! Surveillance-friendly invariant: never places a parent. Modifies
//! existing bracket children only, on brackets the human already
//! confirmed via the take-setup modal. The IBKR-side modify is a
//! `place_order` call against the existing stop's order id with a
//! new `aux_price`; the OCA group + parent linkage stay intact, so
//! the bracket continues to behave as one unit.
//!
//! Callers wire this in `lib.rs::run` once `OrderTicket` and the
//! `BracketGroupStore` are available, then call `spawn()` to start
//! the background poll loop. Tests exercise the math through
//! `revise_one_bracket` and the mock modifier; the loop itself is
//! a thin `tokio::time::interval` driver.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{info, warn};

use crate::ibkr::client::StreamHandle;
use crate::ibkr::error::IbkrError;
use crate::ibkr::types::{ModifyStopRequest, OrderAction};
use crate::services::order_ticket::{
    BracketGroupRecord, BracketGroupStore, BracketModifier, BracketStatus,
};
use crate::services::tracker_service::TrackerService;
use crate::storage::error::StorageError;
use crate::strategies::exits::{
    chandelier_stop, has_reached_r, time_stop, updated_extreme, ChandelierState, ExitPlan,
    TrailKind,
};
use crate::strategies::Direction;
use crate::services::quote_service::QuoteService;
use crate::utils::market_calendar;

#[cfg(test)]
mod tests;

/// Default poll cadence during RTH. Master decision: every 60 seconds
/// — enough time for IBKR rate-limit headroom, fast enough to catch
/// the typical breakout's intraday range expansion.
pub const DEFAULT_RTH_POLL_SECS: u64 = 60;

/// Default poll cadence outside RTH — used only for time-stop checks
/// (which fire at session boundaries). Master decision: every 5 min.
pub const DEFAULT_NON_RTH_POLL_SECS: u64 = 300;

/// Status surface returned by `bracket_reviser_status`. Renders the
/// per-bracket trail snapshot the trader sees in the UI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BracketReviserSnapshot {
    pub parent_order_id: i32,
    pub setup_id: i64,
    pub symbol: String,
    pub direction: String,
    pub status: BracketStatus,
    pub stop_price: f64,
    pub trail_state: Option<ChandelierState>,
    pub time_stop_remaining_days: Option<u32>,
}

/// Decision the reviser made for a single bracket on a single poll.
/// Returned from `revise_one_bracket` so tests can assert the
/// expected behavior without depending on log messages.
#[derive(Debug, Clone, PartialEq)]
pub enum ReviseDecision {
    /// No trail spec on the plan, or trail not yet activated.
    Skipped,
    /// Computed a new chandelier stop and submitted a modify call.
    StopRaised {
        old_stop: f64,
        new_stop: f64,
    },
    /// Computed stop matches existing stop (within 1 cent) — modify
    /// short-circuited.
    NoChange,
    /// BE-move fired: stop pulled up to entry.
    BreakEvenMove {
        new_stop: f64,
    },
    /// Time-stop deadline elapsed; reviser flagged the bracket for
    /// closure (the actual market-close is deferred to the operator
    /// for now — logged as warn so the trader can act).
    TimeStopElapsed,
}

#[derive(Debug, Error)]
pub enum BracketReviserError {
    #[error("storage: {0}")]
    Storage(#[from] StorageError),
    #[error("ibkr: {0}")]
    Ibkr(#[from] IbkrError),
}

pub type Result<T> = std::result::Result<T, BracketReviserError>;

/// One observation handed to the reviser per bracket per poll. In
/// production sourced from a quote stream; in tests hand-rolled. The
/// shape is small on purpose — the reviser is pure math + state, and
/// fetching live observations is a separate seam.
#[derive(Debug, Clone, Copy)]
pub struct PriceObservation {
    /// Most recent extreme price relevant to the trail: high since
    /// last poll for longs, low for shorts. The reviser uses this to
    /// update the chandelier high-water-mark.
    pub extreme_price: f64,
    /// Most recent traded price. Used for the BE-move and trail-
    /// activation comparisons that compare against entry instead of
    /// the trail's high-water.
    pub current_price: f64,
}

/// Trait seam so the reviser can pull a fresh observation per poll
/// without being coupled to a specific quote provider. Production
/// wires `IbkrClient`; tests hand a programmable stub.
#[async_trait::async_trait]
pub trait QuoteSource: Send + Sync {
    async fn observe(&self, symbol: &str) -> std::result::Result<PriceObservation, IbkrError>;
}

/// Production `QuoteSource` that pulls a live snapshot via the shared
/// `QuoteService`. Both `extreme_price` and `current_price` come from
/// `Quote.last_price`; the reviser's running high-water-mark
/// accumulates the intraday extreme across polls. Master gotcha:
/// gap-throughs are accepted (the chandelier can't move a stop the
/// market has already crossed).
pub struct IbkrQuoteSource {
    quotes: Arc<QuoteService>,
}

impl IbkrQuoteSource {
    pub fn new(quotes: Arc<QuoteService>) -> Self {
        Self { quotes }
    }
}

#[async_trait::async_trait]
impl QuoteSource for IbkrQuoteSource {
    async fn observe(&self, symbol: &str) -> std::result::Result<PriceObservation, IbkrError> {
        let q = self.quotes.fetch_quote(symbol).await?;
        let last = q.last_price.unwrap_or(f64::NAN);
        Ok(PriceObservation {
            extreme_price: last,
            current_price: last,
        })
    }
}

/// The service. Cheap to clone — internal state is `Arc`s.
#[derive(Clone)]
pub struct BracketReviser {
    bracket_store: Arc<BracketGroupStore>,
    tracker: Arc<TrackerService>,
    modifier: Arc<dyn BracketModifier>,
    quotes: Arc<dyn QuoteSource>,
    rth_interval: Duration,
    non_rth_interval: Duration,
    /// Atomic flag toggled by the `StreamHandle::stop()` so the spawn
    /// loop can exit cleanly. Mirrors the pattern used by the scanner
    /// + EOD scheduler streams.
    shutdown: Arc<AtomicBool>,
}

impl BracketReviser {
    pub fn new(
        bracket_store: Arc<BracketGroupStore>,
        tracker: Arc<TrackerService>,
        modifier: Arc<dyn BracketModifier>,
        quotes: Arc<dyn QuoteSource>,
    ) -> Self {
        Self {
            bracket_store,
            tracker,
            modifier,
            quotes,
            rth_interval: Duration::from_secs(DEFAULT_RTH_POLL_SECS),
            non_rth_interval: Duration::from_secs(DEFAULT_NON_RTH_POLL_SECS),
            shutdown: Arc::new(AtomicBool::new(false)),
        }
    }

    #[allow(dead_code)] // builder reserved for tests + a future settings knob
    pub fn with_rth_interval(mut self, d: Duration) -> Self {
        self.rth_interval = d;
        self
    }

    #[allow(dead_code)] // builder reserved for tests + a future settings knob
    pub fn with_non_rth_interval(mut self, d: Duration) -> Self {
        self.non_rth_interval = d;
        self
    }

    /// Run a single sweep over every open / partial bracket. Returns
    /// the per-bracket decisions for assertion in tests.
    pub async fn run_sweep(&self) -> Result<Vec<(i32, ReviseDecision)>> {
        let active = self
            .bracket_store
            .list_by_statuses(vec![BracketStatus::Open, BracketStatus::Partial])
            .await?;
        let mut out = Vec::with_capacity(active.len());
        for bracket in active {
            match self.revise_one_bracket(&bracket).await {
                Ok(decision) => out.push((bracket.parent_order_id, decision)),
                Err(e) => {
                    warn!(
                        "bracket_reviser: revise_one_bracket failed for #{}: {e}",
                        bracket.parent_order_id
                    );
                }
            }
        }
        Ok(out)
    }

    /// Per-bracket decision logic. Pure-ish: the only side-effects
    /// are persisting the trail state and submitting the IBKR
    /// modify. Returns the decision so tests can pin behavior.
    pub async fn revise_one_bracket(
        &self,
        bracket: &BracketGroupRecord,
    ) -> Result<ReviseDecision> {
        // Pull the persisted exit plan. NULL plan column → pre-P7
        // bracket → no trail logic to apply. Returns Skipped so the
        // sweep keeps moving.
        let plan = match self.tracker.get_setup_exit_plan(bracket.setup_id).await {
            Ok(Some(p)) => p,
            Ok(None) => return Ok(ReviseDecision::Skipped),
            Err(e) => {
                warn!(
                    "bracket_reviser: get_setup_exit_plan failed for setup#{}: {e}",
                    bracket.setup_id
                );
                return Ok(ReviseDecision::Skipped);
            }
        };

        // Time-stop gate — cheap, runs even pre-trail-activation.
        if let Some(spec) = plan.time_stop.as_ref() {
            if time_stop::has_elapsed(Utc::now(), bracket.placed_at, spec) {
                warn!(
                    "bracket_reviser: time-stop elapsed for bracket#{} setup#{} \
                     ({} BD horizon) — flag for manual close",
                    bracket.parent_order_id, bracket.setup_id, spec.max_trading_days
                );
                return Ok(ReviseDecision::TimeStopElapsed);
            }
        }

        let trail = match plan.trail.as_ref() {
            Some(t) if matches!(t.kind, TrailKind::Chandelier) => t,
            _ => return Ok(ReviseDecision::Skipped),
        };

        let direction = parse_direction(&bracket.direction).unwrap_or(Direction::Long);
        let entry_price = bracket.entry_limit_cents as f64 / 100.0;
        let original_stop = bracket.stop_price_cents as f64 / 100.0;
        let r_distance = (entry_price - original_stop).abs();
        if r_distance == 0.0 {
            return Ok(ReviseDecision::Skipped);
        }

        let observation = match self.quotes.observe(&bracket.symbol).await {
            Ok(o) => o,
            Err(e) => {
                warn!(
                    "bracket_reviser: quote fetch failed for {} ({}): {e}",
                    bracket.symbol, bracket.parent_order_id
                );
                return Ok(ReviseDecision::Skipped);
            }
        };

        let prev_state = self
            .bracket_store
            .get_trail_state(bracket.parent_order_id)
            .await?
            .unwrap_or_else(|| ChandelierState::new(original_stop));

        let new_extreme = updated_extreme(direction, prev_state.extreme_price, observation.extreme_price);

        // BE-move fires once at 1R profit, independent of trail
        // activation. Only moves UP the stop (long) / DOWN (short).
        let mut be_just_fired = false;
        let mut be_moved = prev_state.be_moved;
        let mut current_stop = prev_state.current_stop_price;
        if let Some(r_threshold) = trail.move_to_break_even_at_r {
            if !be_moved
                && has_reached_r(
                    direction,
                    entry_price,
                    r_distance,
                    observation.current_price,
                    r_threshold,
                )
            {
                let candidate = entry_price;
                let moved = match direction {
                    Direction::Long => current_stop.max(candidate),
                    Direction::Short => current_stop.min(candidate),
                };
                if (moved - current_stop).abs() > 0.005 {
                    current_stop = moved;
                    be_just_fired = true;
                }
                be_moved = true;
            }
        }

        // Trail activation: by default fires once the first ladder
        // rung's profit threshold is crossed. We approximate
        // "first-rung filled" with "current price has reached the
        // first-rung price" — without a fill stream, the reviser
        // can't know about partial fills, but it CAN observe that
        // price has traded through. The activation flag persists
        // once true.
        let mut activated = prev_state.activated;
        if !activated && trail_should_activate(trail.activate_after_label.as_deref(), &plan, direction, observation.current_price) {
            activated = true;
        }

        let mut new_stop = current_stop;
        let mut decision = if be_just_fired {
            ReviseDecision::BreakEvenMove {
                new_stop: current_stop,
            }
        } else {
            ReviseDecision::NoChange
        };

        if activated {
            let atr = plan.atr_at_signal.unwrap_or(0.0);
            let chandelier = chandelier_stop(
                direction,
                new_extreme,
                atr,
                trail.atr_multiple,
                current_stop,
            );
            if (chandelier - current_stop).abs() > 0.005 {
                decision = ReviseDecision::StopRaised {
                    old_stop: current_stop,
                    new_stop: chandelier,
                };
                new_stop = chandelier;
            }
        }

        // Persist + modify only when the stop actually moved or the
        // BE move just fired. Quote-only updates that don't move the
        // stop still re-write the extreme_price so the next poll
        // sees the up-to-date high-water-mark.
        let stop_changed = (new_stop - prev_state.current_stop_price).abs() > 0.005;
        let new_state = ChandelierState {
            extreme_price: new_extreme,
            current_stop_price: new_stop,
            activated,
            be_moved,
            last_modify_at: if stop_changed { Some(Utc::now()) } else { prev_state.last_modify_at },
        };
        self.bracket_store
            .update_trail_state(bracket.parent_order_id, new_stop, &new_state)
            .await?;

        if stop_changed {
            // Re-check status — abort if a fill landed since we
            // pulled the bracket. Master gotcha: race between
            // modify and fill on the trail-target.
            if let Some(latest) = self
                .bracket_store
                .get(bracket.parent_order_id)
                .await
                .ok()
                .flatten()
            {
                if !matches!(latest.last_status, BracketStatus::Open | BracketStatus::Partial) {
                    info!(
                        "bracket_reviser: aborted modify for #{} — status flipped to {:?}",
                        bracket.parent_order_id, latest.last_status
                    );
                    return Ok(ReviseDecision::NoChange);
                }
            }

            let stop_action = match direction {
                Direction::Long => OrderAction::Sell,
                Direction::Short => OrderAction::Buy,
            };
            let req = ModifyStopRequest {
                stop_order_id: bracket.stop_order_id,
                parent_id: bracket.parent_order_id,
                symbol: bracket.symbol.clone(),
                action: stop_action,
                qty: f64::from(bracket.parent_qty),
                new_stop_price: new_stop,
                oca_group: format!("br-{}", bracket.parent_order_id),
            };
            if let Err(e) = self.modifier.modify_stop(req).await {
                warn!(
                    "bracket_reviser: modify_stop failed for bracket#{}: {e}",
                    bracket.parent_order_id
                );
            }
        }

        Ok(decision)
    }

    /// Snapshot every active bracket for the status surface.
    pub async fn snapshot(&self) -> Result<Vec<BracketReviserSnapshot>> {
        let active = self
            .bracket_store
            .list_by_statuses(vec![BracketStatus::Open, BracketStatus::Partial])
            .await?;
        let now = Utc::now();
        let mut out = Vec::with_capacity(active.len());
        for bracket in active {
            let trail_state = self
                .bracket_store
                .get_trail_state(bracket.parent_order_id)
                .await
                .ok()
                .flatten();
            let plan = self
                .tracker
                .get_setup_exit_plan(bracket.setup_id)
                .await
                .ok()
                .flatten();
            let time_stop_remaining = plan.as_ref().and_then(|p| p.time_stop.as_ref()).map(|spec| {
                time_stop::days_remaining(now, bracket.placed_at, spec)
            });
            out.push(BracketReviserSnapshot {
                parent_order_id: bracket.parent_order_id,
                setup_id: bracket.setup_id,
                symbol: bracket.symbol.clone(),
                direction: bracket.direction.clone(),
                status: bracket.last_status,
                stop_price: bracket.stop_price_cents as f64 / 100.0,
                trail_state,
                time_stop_remaining_days: time_stop_remaining,
            });
        }
        Ok(out)
    }

    /// Spawn the poll loop. Returns a [`StreamHandle`] callers can
    /// store on `IbkrState` and `stop()` on shutdown. Mirrors the
    /// shape used by the auto-scanner / EOD scheduler so the
    /// composition pattern stays uniform.
    pub fn spawn(self: Arc<Self>) -> StreamHandle {
        let shutdown = Arc::clone(&self.shutdown);
        let join = tokio::spawn(async move {
            loop {
                if self.shutdown.load(Ordering::Relaxed) {
                    info!("bracket_reviser: shutdown flag set — exiting poll loop");
                    return;
                }
                let now = Utc::now();
                let interval = if market_calendar::is_rth_open(now) {
                    self.rth_interval
                } else {
                    self.non_rth_interval
                };
                if let Err(e) = self.run_sweep().await {
                    warn!("bracket_reviser: run_sweep failed: {e}");
                }
                tokio::time::sleep(interval).await;
            }
        });
        StreamHandle::new("BracketReviser", shutdown, join)
    }
}

fn parse_direction(s: &str) -> Option<Direction> {
    match s {
        "long" => Some(Direction::Long),
        "short" => Some(Direction::Short),
        _ => None,
    }
}

/// Trail activates when the configured rung-label has been reached
/// (price has traded through the rung price). When no label is
/// configured, activates immediately.
fn trail_should_activate(
    activate_after_label: Option<&str>,
    plan: &ExitPlan,
    direction: Direction,
    current_price: f64,
) -> bool {
    let Some(label) = activate_after_label else {
        return true;
    };
    let target = match plan.targets.iter().find(|t| t.label == label) {
        Some(t) => t,
        None => return false,
    };
    if !current_price.is_finite() {
        return false;
    }
    match direction {
        Direction::Long => current_price >= target.price,
        Direction::Short => current_price <= target.price,
    }
}
