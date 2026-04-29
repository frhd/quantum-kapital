//! Phase 13 — End-of-day scheduler.
//!
//! Owns a long-running tokio task that wakes once a minute, checks whether
//! the wall clock is inside a 5-minute window starting at 16:05 ET on a US
//! equity trading day, and — exactly once per trading day — triggers a full
//! tracker sweep:
//!
//!   1. `TrackerRunner::run_all()` — re-evaluate every active watchlist row.
//!   2. `TrackerStateMachine::expire_ttls(now)` — flip stale `in_play` /
//!      `cool_down` rows back to `Watching`.
//!   3. Emit `AppEvent::MorningPackReady { date }`. The payload is empty
//!      for now; Phase 20's daily ranker fills it in.
//!
//! Test seam: a `Clock` enum (mirroring `tracker_state_machine::Clock`) lets
//! tests pin the wall clock and call [`EodScheduler::tick`] directly without
//! waiting for the 60-second loop. `last_run_date` is exposed for read-only
//! assertions and de-duplication checks.

// Production callers: Tauri commands + `lib.rs::run`. Phase 20 will read
// `last_run_date` from the LLM ranker; until then a couple of helpers are
// only exercised by tests.
#![allow(dead_code)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Datelike, FixedOffset, NaiveDate, NaiveTime, Utc, Weekday};
use tokio::sync::RwLock;
use tracing::{info, warn};

use crate::events::{AppEvent, EventEmitter};
use crate::ibkr::client::StreamHandle;
use crate::services::tracker_runner::{RunResult, TrackerRunner};
use crate::services::tracker_state_machine::TrackerStateMachine;
use crate::utils::market_calendar::is_holiday;

#[cfg(test)]
mod tests;

/// Polling interval. The loop just samples the wall clock — the actual EOD
/// work is gated by the 5-minute window check inside `tick`.
const TICK_INTERVAL: Duration = Duration::from_secs(60);

/// Window start: 16:05 ET. The five-minute slack absorbs any delay between
/// `tick` ticks and the first check after market close.
const WINDOW_START: (u32, u32) = (16, 5);
/// Window end (exclusive): 16:10 ET. Past this and the tick is a no-op even
/// if `last_run_date` says we haven't run today (we'll catch it tomorrow).
const WINDOW_END: (u32, u32) = (16, 10);

fn et_offset() -> FixedOffset {
    FixedOffset::west_opt(5 * 3600).expect("ET offset is valid")
}

/// Clock seam — production wires `Real` (`Utc::now()`), tests pin a fixed
/// instant so the EOD-window math is deterministic. Mirrors the equivalent
/// type in `tracker_state_machine` rather than sharing it because the two
/// services have independent test-time pinning needs.
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

/// Outcome of a tick that actually fired the EOD work — returned for
/// observability + tests.
#[derive(Debug, Clone)]
pub struct EodTickOutcome {
    /// ET trading-day date that ran.
    pub date: NaiveDate,
    /// Per-symbol results from `TrackerRunner::run_all`.
    pub run_results: Vec<RunResult>,
    /// Number of rows whose TTLs expired and were swept back to `Watching`.
    pub expired: usize,
}

#[derive(Clone)]
pub struct EodScheduler {
    runner: Arc<TrackerRunner>,
    state_machine: Arc<TrackerStateMachine>,
    emitter: Arc<EventEmitter>,
    /// Last ET trading-day on which the EOD sweep ran. Guards against
    /// double-runs inside the 5-minute window.
    last_run_date: Arc<RwLock<Option<NaiveDate>>>,
    clock: Arc<RwLock<Clock>>,
}

impl EodScheduler {
    pub fn new(
        runner: Arc<TrackerRunner>,
        state_machine: Arc<TrackerStateMachine>,
        emitter: Arc<EventEmitter>,
    ) -> Self {
        Self {
            runner,
            state_machine,
            emitter,
            last_run_date: Arc::new(RwLock::new(None)),
            clock: Arc::new(RwLock::new(Clock::Real)),
        }
    }

    #[cfg(test)]
    pub fn with_clock(
        runner: Arc<TrackerRunner>,
        state_machine: Arc<TrackerStateMachine>,
        emitter: Arc<EventEmitter>,
        clock: Clock,
    ) -> Self {
        Self {
            runner,
            state_machine,
            emitter,
            last_run_date: Arc::new(RwLock::new(None)),
            clock: Arc::new(RwLock::new(clock)),
        }
    }

    pub async fn last_run_date(&self) -> Option<NaiveDate> {
        *self.last_run_date.read().await
    }

    #[cfg(test)]
    pub async fn set_clock(&self, clock: Clock) {
        *self.clock.write().await = clock;
    }

    async fn now(&self) -> DateTime<Utc> {
        self.clock.read().await.now()
    }

    /// Run one scheduling tick. Returns `Ok(Some(_))` when the EOD work
    /// actually fired, `Ok(None)` when this tick was a no-op (outside the
    /// window, weekend / holiday, or already-run today).
    pub async fn tick(&self) -> Result<Option<EodTickOutcome>, String> {
        let now = self.now().await;
        let et = now.with_timezone(&et_offset());
        let date = et.date_naive();

        if matches!(date.weekday(), Weekday::Sat | Weekday::Sun) || is_holiday(date) {
            return Ok(None);
        }

        let window_start =
            NaiveTime::from_hms_opt(WINDOW_START.0, WINDOW_START.1, 0).expect("valid window start");
        let window_end =
            NaiveTime::from_hms_opt(WINDOW_END.0, WINDOW_END.1, 0).expect("valid window end");
        let time = et.time();
        if time < window_start || time >= window_end {
            return Ok(None);
        }

        if *self.last_run_date.read().await == Some(date) {
            return Ok(None);
        }

        info!("EOD scheduler firing for {date}");

        let run_results = self
            .runner
            .run_all()
            .await
            .map_err(|e| format!("run_all failed: {e}"))?;

        let expired = self
            .state_machine
            .expire_ttls(now)
            .await
            .map_err(|e| format!("expire_ttls failed: {e}"))?;

        if let Err(e) = self
            .emitter
            .emit(AppEvent::MorningPackReady {
                date: date.format("%Y-%m-%d").to_string(),
            })
            .await
        {
            // Emitter just logs when there's no app handle (e.g. tests) —
            // demote to warn so it doesn't break the schedule.
            warn!("MorningPackReady emit failed: {e}");
        }

        *self.last_run_date.write().await = Some(date);

        Ok(Some(EodTickOutcome {
            date,
            run_results,
            expired,
        }))
    }

    /// Spawn the tick loop on the tokio runtime. Returns a [`StreamHandle`]
    /// that the caller stores on `IbkrState::eod_handle` and `stop()`s on
    /// shutdown — same pattern as `start_scanner_stream` /
    /// `start_daily_pnl_stream` in `ibkr/client.rs`.
    pub fn spawn(self: Arc<Self>) -> StreamHandle {
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_task = Arc::clone(&shutdown);
        let scheduler = Arc::clone(&self);

        let join = tokio::spawn(async move {
            let mut ticker = tokio::time::interval(TICK_INTERVAL);
            // Skip the immediate first-tick burst so we don't run inside
            // the same 60-second slot the caller starts us in.
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
                    warn!("EOD tick failed: {e}");
                }
            }
            info!("EOD scheduler stopped");
        });

        StreamHandle::new("EOD scheduler", shutdown, join)
    }
}
