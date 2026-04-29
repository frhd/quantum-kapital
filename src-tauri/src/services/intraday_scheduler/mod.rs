//! Phase 14 — intraday scheduler.
//!
//! Owns a long-running tokio task that wakes every 5 minutes during US
//! equity RTH (09:30–16:00 ET, weekdays excluding holidays) and, for
//! every ticker the state machine flags as in-play or setup-active:
//!
//!   1. Calls [`TrackerRunner::run_for`] to refresh bars/news and
//!      re-evaluate detectors. Persists any new hits the same way the
//!      EOD sweep does.
//!   2. For tickers in `SetupActive`, walks every persisted setup row
//!      with `status = 'active'` and asks the
//!      [`DecayWatcher`](crate::services::decay_watcher::DecayWatcher)
//!      whether the thesis still holds. When the watcher returns
//!      `still_valid = false`, the scheduler hands the result to
//!      [`TrackerStateMachine::mark_invalidated`] which flips the
//!      ticker into `CoolDown` once no other active setups remain.
//!
//! Test seam: a `Clock` enum (mirroring `eod_scheduler::Clock`) lets
//! tests pin the wall clock and call [`IntradayScheduler::tick`]
//! directly without spawning the 5-minute loop. The internal
//! `last_tick_at` cursor enforces the 5-minute cadence even across
//! manual `tick` calls.
//!
//! Phase 18 will replace [`crate::services::decay_watcher::DecayWatcherStub`]
//! with a real Anthropic-backed implementation; no change to this
//! scheduler is required.

// Production callers: Tauri commands + `lib.rs::run`. Phase 15/18 will
// consume more of the public surface; until then a couple of helpers are
// exercised only by tests.
#![allow(dead_code)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use tokio::sync::RwLock;
use tracing::{info, warn};

use crate::ibkr::client::StreamHandle;
use crate::ibkr::types::tracker::{SetupStatus, TrackerStatus};
use crate::services::decay_watcher::{DecayOutcome, DecayWatcher};
use crate::services::tracker_runner::{RunResult, TrackerRunner};
use crate::services::tracker_service::TrackerService;
use crate::services::tracker_state_machine::TrackerStateMachine;
use crate::utils::market_calendar::is_rth_open;

#[cfg(test)]
mod tests;

/// Default cadence: 5 minutes between ticks. Configurable via
/// `AppConfig.intraday_tick_interval_secs`.
pub const DEFAULT_TICK_INTERVAL: Duration = Duration::from_secs(300);

/// Clock seam — production wires `Real` (`Utc::now()`); tests pin a
/// fixed instant so the RTH-window math + cadence cursor are
/// deterministic. Mirrors the equivalent type in `eod_scheduler`.
#[derive(Clone, Debug)]
pub enum Clock {
    Real,
    Fixed(DateTime<Utc>),
}

impl Clock {
    fn now(&self) -> DateTime<Utc> {
        match self {
            Clock::Real => Utc::now(),
            Clock::Fixed(dt) => *dt,
        }
    }
}

/// Outcome of a tick that actually fired — returned for observability
/// and tests. A no-op tick (outside RTH, before the cadence cursor, or
/// no in-play tickers) returns `Ok(None)`.
#[derive(Debug, Clone)]
pub struct IntradayTickOutcome {
    /// Symbols the scheduler iterated this tick (i.e. the snapshot of
    /// `state_machine.active_in_play_symbols`). Empty when the
    /// watchlist had no in-play rows.
    pub processed_symbols: Vec<String>,
    /// Per-symbol detector outcomes from `runner.run_for`.
    pub run_results: Vec<RunResult>,
    /// Setup ids the decay-watcher flagged as invalid this tick.
    pub invalidated_setup_ids: Vec<i64>,
    /// Setup ids the decay-watcher reported a target hit on this tick.
    pub completed_setup_ids: Vec<i64>,
}

#[derive(Clone)]
pub struct IntradayScheduler {
    runner: Arc<TrackerRunner>,
    state_machine: Arc<TrackerStateMachine>,
    tracker: Arc<TrackerService>,
    decay_watcher: Arc<dyn DecayWatcher>,
    clock: Arc<RwLock<Clock>>,
    tick_interval: Duration,
    /// Last clock value at which `tick` actually ran. Used to enforce
    /// the configured cadence even when callers spam `tick` (e.g.
    /// tests, or a future "run intraday now" command).
    last_tick_at: Arc<RwLock<Option<DateTime<Utc>>>>,
}

impl IntradayScheduler {
    pub fn new(
        runner: Arc<TrackerRunner>,
        state_machine: Arc<TrackerStateMachine>,
        tracker: Arc<TrackerService>,
        decay_watcher: Arc<dyn DecayWatcher>,
        tick_interval: Duration,
    ) -> Self {
        Self {
            runner,
            state_machine,
            tracker,
            decay_watcher,
            clock: Arc::new(RwLock::new(Clock::Real)),
            tick_interval,
            last_tick_at: Arc::new(RwLock::new(None)),
        }
    }

    #[cfg(test)]
    pub fn with_clock(
        runner: Arc<TrackerRunner>,
        state_machine: Arc<TrackerStateMachine>,
        tracker: Arc<TrackerService>,
        decay_watcher: Arc<dyn DecayWatcher>,
        tick_interval: Duration,
        clock: Clock,
    ) -> Self {
        Self {
            runner,
            state_machine,
            tracker,
            decay_watcher,
            clock: Arc::new(RwLock::new(clock)),
            tick_interval,
            last_tick_at: Arc::new(RwLock::new(None)),
        }
    }

    #[cfg(test)]
    pub async fn set_clock(&self, clock: Clock) {
        *self.clock.write().await = clock;
    }

    pub async fn last_tick_at(&self) -> Option<DateTime<Utc>> {
        *self.last_tick_at.read().await
    }

    async fn now(&self) -> DateTime<Utc> {
        self.clock.read().await.now()
    }

    /// Run one scheduling tick. Returns `Ok(Some(_))` when the
    /// scheduler actually iterated the in-play watchlist, `Ok(None)`
    /// when the tick was a no-op (outside RTH, holiday/weekend, or
    /// inside the cadence window).
    pub async fn tick(&self) -> Result<Option<IntradayTickOutcome>, String> {
        let now = self.now().await;

        if !is_rth_open(now) {
            return Ok(None);
        }

        if let Some(last) = *self.last_tick_at.read().await {
            if now.signed_duration_since(last).to_std().unwrap_or_default() < self.tick_interval {
                return Ok(None);
            }
        }

        let symbols = self
            .state_machine
            .active_in_play_symbols()
            .await
            .map_err(|e| format!("active_in_play_symbols failed: {e}"))?;

        let mut run_results = Vec::with_capacity(symbols.len());
        let mut invalidated_setup_ids = Vec::new();
        let mut completed_setup_ids = Vec::new();

        for symbol in &symbols {
            // Step 1: re-run detectors against fresh bars/news. Errors
            // are surfaced inside the RunResult so a single bad
            // ticker can't tear down the loop.
            let run_result = match self.runner.run_for(symbol).await {
                Ok(setups) => RunResult {
                    symbol: symbol.clone(),
                    setups,
                    error: None,
                },
                Err(e) => {
                    warn!("intraday run_for failed for {symbol}: {e}");
                    RunResult {
                        symbol: symbol.clone(),
                        setups: Vec::new(),
                        error: Some(e.to_string()),
                    }
                }
            };
            run_results.push(run_result);

            // Step 2: decay-watcher pass. Only fire for tickers
            // currently in `SetupActive` — `InPlay` rows have no
            // active setups to evaluate yet.
            let row = match self.tracker.get(symbol).await {
                Ok(Some(r)) => r,
                Ok(None) => continue,
                Err(e) => {
                    warn!("tracker.get failed for {symbol} during intraday tick: {e}");
                    continue;
                }
            };
            if !matches!(row.status, TrackerStatus::SetupActive) {
                continue;
            }

            let setups = match self.tracker.list_setups(Some(symbol), None).await {
                Ok(rows) => rows,
                Err(e) => {
                    warn!("list_setups failed for {symbol} during intraday tick: {e}");
                    continue;
                }
            };
            for setup in setups
                .into_iter()
                .filter(|s| matches!(s.status, SetupStatus::Active))
            {
                let setup_id = setup.id;
                let decision = self.decay_watcher.check(&setup).await;
                match decision.outcome {
                    DecayOutcome::StillValid | DecayOutcome::Skipped => continue,
                    DecayOutcome::Invalidated | DecayOutcome::ThesisChanged => {
                        let reason = decision
                            .reason
                            .as_deref()
                            .unwrap_or("decay-watcher invalidation");
                        if let Err(e) = self.state_machine.mark_invalidated(setup_id, reason).await
                        {
                            warn!("mark_invalidated failed for setup#{setup_id}: {e}");
                            continue;
                        }
                        invalidated_setup_ids.push(setup_id);
                    }
                    DecayOutcome::TargetHit => {
                        if let Err(e) = self.state_machine.mark_completed(setup_id).await {
                            warn!("mark_completed failed for setup#{setup_id}: {e}");
                            continue;
                        }
                        completed_setup_ids.push(setup_id);
                    }
                }
            }
        }

        *self.last_tick_at.write().await = Some(now);

        info!(
            "intraday tick processed {} symbol(s), invalidated {} setup(s), completed {} setup(s)",
            symbols.len(),
            invalidated_setup_ids.len(),
            completed_setup_ids.len()
        );

        Ok(Some(IntradayTickOutcome {
            processed_symbols: symbols,
            run_results,
            invalidated_setup_ids,
            completed_setup_ids,
        }))
    }

    /// Spawn the tick loop. Returns a [`StreamHandle`] suitable for
    /// `IbkrState::intraday_handle`. The polling interval matches
    /// `tick_interval` but the actual gating is done inside `tick` —
    /// the loop just samples the clock.
    pub fn spawn(self: Arc<Self>) -> StreamHandle {
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_task = Arc::clone(&shutdown);
        let scheduler = Arc::clone(&self);
        // Poll at most once a minute so that we still notice RTH-open /
        // first-tick boundaries quickly even when `tick_interval` is
        // configured larger than that.
        let poll = scheduler.tick_interval.min(Duration::from_secs(60));

        let join = tokio::spawn(async move {
            let mut ticker = tokio::time::interval(poll);
            // Skip the immediate first-tick burst so we don't
            // double-fire in the same wall-clock slot we started in.
            ticker.tick().await;

            loop {
                if shutdown_task.load(Ordering::Relaxed) {
                    break;
                }
                ticker.tick().await;
                if shutdown_task.load(Ordering::Relaxed) {
                    break;
                }
                if let Err(e) = scheduler.tick().await {
                    warn!("intraday tick failed: {e}");
                }
            }
            info!("intraday scheduler stopped");
        });

        StreamHandle::new("intraday scheduler", shutdown, join)
    }
}
