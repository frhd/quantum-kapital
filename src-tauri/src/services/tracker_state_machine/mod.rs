//! Phase 12 — tracker status state machine.
//!
//! Codifies the lifecycle that drives intraday eligibility and prevents the
//! same setup from re-firing forever:
//!
//! ```text
//! watching     ──[scanner hit | manual flag]──────────────> in_play (TTL 3 trading days)
//! watching     ──[detector hit]───────────────────────────> setup_active
//! in_play      ──[detector hit]───────────────────────────> setup_active
//! setup_active ──[invalidated | completed | manual stop]──> cool_down (TTL 5 trading days)
//! cool_down    ──[TTL expires]────────────────────────────> watching
//! in_play      ──[TTL expires]────────────────────────────> watching
//! ```
//!
//! Out of scope here: the schedulers that drive `expire_ttls` (Phase 13's EOD
//! sweep) and the LLM decay-watcher that fires `mark_invalidated`
//! (Phase 18). This module just owns the transition rules + the SQL.

// Most of the public surface is exercised only via tests until Phase 13/14
// schedulers + Phase 18's LLM decay-watcher land. The TDD API contract is
// intentionally stable now.
#![allow(dead_code)]

use std::sync::Arc;

use chrono::{DateTime, Utc};
use thiserror::Error;
use tracing::warn;

use crate::events::{AppEvent, EventEmitter};
use crate::ibkr::types::tracker::{AlertKind, SetupStatus, TrackerStatus};
use crate::services::alerts::record_alert;
use crate::services::tracker_service::{TrackerError, TrackerService};
use crate::storage::error::StorageError;
use crate::storage::Db;
use crate::utils::market_calendar::trading_days_after_close;

#[cfg(test)]
mod tests;

/// In-play TTL — how long after a scanner hit / detector hit a ticker stays
/// eligible for intraday-cadence checks. 3 trading days matches the locked-in
/// disciplined-swing profile in `impl.md`.
pub const IN_PLAY_TRADING_DAYS: u32 = 3;

/// Cool-down TTL — how long a ticker rests after a setup invalidates or
/// completes before it returns to `Watching` and is eligible to fire again.
pub const COOL_DOWN_TRADING_DAYS: u32 = 5;

#[derive(Error, Debug)]
pub enum StateMachineError {
    #[error("tracker: {0}")]
    Tracker(#[from] TrackerError),
    #[error("storage: {0}")]
    Storage(#[from] StorageError),
    #[error("setup#{0} not found")]
    SetupNotFound(i64),
}

pub type Result<T> = std::result::Result<T, StateMachineError>;

/// Injectable clock — production wires `Real` (which calls `Utc::now()`),
/// tests pin a `Fixed` instant so trading-day math is deterministic.
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

#[derive(Clone)]
pub struct TrackerStateMachine {
    db: Arc<Db>,
    tracker: Arc<TrackerService>,
    emitter: Arc<EventEmitter>,
    clock: Clock,
}

impl TrackerStateMachine {
    pub fn new(db: Arc<Db>, tracker: Arc<TrackerService>, emitter: Arc<EventEmitter>) -> Self {
        Self {
            db,
            tracker,
            emitter,
            clock: Clock::Real,
        }
    }

    #[cfg(test)]
    pub fn with_clock(
        db: Arc<Db>,
        tracker: Arc<TrackerService>,
        emitter: Arc<EventEmitter>,
        clock: Clock,
    ) -> Self {
        Self {
            db,
            tracker,
            emitter,
            clock,
        }
    }

    /// Promote a `Watching` (or already in-play) ticker after the scanner
    /// flags it. The optional `meta` is folded into the row's `source_meta`
    /// JSON column when provided so the latest scanner snapshot survives.
    /// No-ops on `SetupActive` / `CoolDown` rows — the caller already has
    /// a hotter signal in flight.
    pub async fn record_scanner_hit(
        &self,
        symbol: &str,
        meta: Option<serde_json::Value>,
    ) -> Result<()> {
        self.promote_to_in_play(symbol, meta).await
    }

    /// User-driven flag from the UI ("treat this as in-play"). Same effect
    /// as a scanner hit but without source-meta.
    pub async fn record_manual_flag(&self, symbol: &str) -> Result<()> {
        self.promote_to_in_play(symbol, None).await
    }

    /// Promote a ticker to `SetupActive` once a detector emits a candidate.
    /// Works from any state except `CoolDown`, which is intentionally
    /// excluded by the runner upstream — defensive `warn!` here so a stale
    /// caller surfaces in logs.
    pub async fn on_setup_detected(&self, symbol: &str, _setup_id: i64) -> Result<()> {
        let symbol_norm = symbol.to_uppercase();
        let row = match self.tracker.get(&symbol_norm).await? {
            Some(r) => r,
            None => {
                warn!("on_setup_detected called for untracked symbol {symbol_norm}");
                return Ok(());
            }
        };
        if matches!(row.status, TrackerStatus::CoolDown) {
            warn!(
                "on_setup_detected called for {symbol_norm} in CoolDown — \
                 ignoring (cool-down must expire first)"
            );
            return Ok(());
        }
        let now = self.clock.now();
        let in_play_until = trading_days_after_close(now, IN_PLAY_TRADING_DAYS);
        self.set_status_and_emit(
            &symbol_norm,
            row.status,
            TrackerStatus::SetupActive,
            Some(in_play_until),
            None,
        )
        .await
    }

    /// Mark a setup as `Invalidated` (stop hit, thesis broken, manual stop).
    /// Only flips the ticker to `CoolDown` when the symbol has no other
    /// `Active` setups remaining — otherwise the ticker stays `SetupActive`
    /// because there's still a live thesis on it.
    pub async fn mark_invalidated(&self, setup_id: i64, reason: &str) -> Result<()> {
        let now = self.clock.now();
        let setup = self
            .tracker
            .update_setup_status(
                setup_id,
                SetupStatus::Invalidated,
                Some(reason.to_string()),
                Some(now),
            )
            .await
            .map_err(|e| match e {
                TrackerError::NotFound(_) => StateMachineError::SetupNotFound(setup_id),
                other => StateMachineError::Tracker(other),
            })?;
        // Emit the invalidation regardless of whether the ticker flips
        // to CoolDown — the frontend cares about per-setup lifecycle,
        // not just per-ticker status.
        let _ = self
            .emitter
            .emit(AppEvent::SetupInvalidated {
                setup_id: setup.id,
                symbol: setup.symbol.clone(),
                reason: reason.to_string(),
            })
            .await;
        // Phase 21: record an `invalidated` alert for the AlertFeed.
        if let Err(e) = record_alert(
            &self.db,
            setup.id,
            AlertKind::Invalidated,
            serde_json::json!({
                "symbol": setup.symbol,
                "reason": reason,
            }),
        )
        .await
        {
            warn!(
                "record_alert(invalidated) failed for setup#{}: {e}",
                setup.id
            );
        }
        self.maybe_enter_cool_down(&setup.symbol, now).await
    }

    /// Mark a setup as `Completed` (target hit). Same cool-down semantics
    /// as `mark_invalidated`.
    pub async fn mark_completed(&self, setup_id: i64) -> Result<()> {
        let now = self.clock.now();
        let setup = self
            .tracker
            .update_setup_status(setup_id, SetupStatus::Completed, None, Some(now))
            .await
            .map_err(|e| match e {
                TrackerError::NotFound(_) => StateMachineError::SetupNotFound(setup_id),
                other => StateMachineError::Tracker(other),
            })?;
        // Phase 21: record a `target_hit` alert for the AlertFeed.
        if let Err(e) = record_alert(
            &self.db,
            setup.id,
            AlertKind::TargetHit,
            serde_json::json!({
                "symbol": setup.symbol,
            }),
        )
        .await
        {
            warn!(
                "record_alert(target_hit) failed for setup#{}: {e}",
                setup.id
            );
        }
        self.maybe_enter_cool_down(&setup.symbol, now).await
    }

    /// Sweep the watchlist for expired TTLs and flip them back to
    /// `Watching`. Both `in_play_until` and `cool_down_until` are checked
    /// in a single SQL UPDATE so the call is atomic. Returns the number
    /// of rows transitioned. Emits `TickerStatusChanged` per flipped row
    /// so the frontend badges clear without a refresh.
    pub async fn expire_ttls(&self, now: DateTime<Utc>) -> Result<usize> {
        let now_unix = now.timestamp();
        // Snapshot the rows that will flip so we can emit per-ticker
        // events after the atomic UPDATE.
        let to_flip: Vec<(String, Option<TrackerStatus>)> = self
            .db
            .with_conn(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT symbol, status FROM tracked_tickers \
                     WHERE (in_play_until IS NOT NULL AND in_play_until <= ?1) \
                        OR (cool_down_until IS NOT NULL AND cool_down_until <= ?1)",
                )?;
                let iter = stmt.query_map(rusqlite::params![now_unix], |row| {
                    let symbol: String = row.get(0)?;
                    let status: String = row.get(1)?;
                    Ok((symbol, status))
                })?;
                let mut out = Vec::new();
                for r in iter {
                    let (symbol, status) = r?;
                    out.push((symbol, TrackerStatus::parse(&status)));
                }
                Ok(out)
            })
            .await?;

        let n = self
            .db
            .with_conn(move |conn| {
                let n = conn.execute(
                    "UPDATE tracked_tickers \
                     SET status = 'watching', in_play_until = NULL, cool_down_until = NULL \
                     WHERE (in_play_until IS NOT NULL AND in_play_until <= ?1) \
                        OR (cool_down_until IS NOT NULL AND cool_down_until <= ?1)",
                    rusqlite::params![now_unix],
                )?;
                Ok(n)
            })
            .await?;

        for (symbol, from) in to_flip {
            if let Some(from) = from {
                if from != TrackerStatus::Watching {
                    let _ = self
                        .emitter
                        .emit(AppEvent::TickerStatusChanged {
                            symbol,
                            from,
                            to: TrackerStatus::Watching,
                        })
                        .await;
                }
            }
        }

        Ok(n)
    }

    /// Symbols whose status warrants intraday-cadence checks.
    /// Phase 14's intraday scheduler will consume this.
    pub async fn active_in_play_symbols(&self) -> Result<Vec<String>> {
        let rows = self
            .db
            .with_conn(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT symbol FROM tracked_tickers \
                     WHERE status IN ('in_play', 'setup_active') \
                     ORDER BY symbol ASC",
                )?;
                let iter = stmt.query_map([], |row| row.get::<_, String>(0))?;
                let mut out = Vec::new();
                for r in iter {
                    out.push(r?);
                }
                Ok(out)
            })
            .await?;
        Ok(rows)
    }

    // ---------------- internals ----------------

    async fn promote_to_in_play(
        &self,
        symbol: &str,
        meta: Option<serde_json::Value>,
    ) -> Result<()> {
        let symbol_norm = symbol.to_uppercase();
        let row = match self.tracker.get(&symbol_norm).await? {
            Some(r) => r,
            None => {
                warn!("promote_to_in_play called for untracked symbol {symbol_norm}");
                return Ok(());
            }
        };
        match row.status {
            TrackerStatus::Watching | TrackerStatus::InPlay => {
                let now = self.clock.now();
                let in_play_until = trading_days_after_close(now, IN_PLAY_TRADING_DAYS);
                self.set_status_and_emit(
                    &symbol_norm,
                    row.status,
                    TrackerStatus::InPlay,
                    Some(in_play_until),
                    None,
                )
                .await?;
                if let Some(meta_value) = meta {
                    self.update_source_meta(&symbol_norm, meta_value).await?;
                }
                Ok(())
            }
            TrackerStatus::SetupActive | TrackerStatus::CoolDown => {
                // A scanner hit on an already-active or cooling-down row
                // is informational; don't overwrite the hotter state.
                Ok(())
            }
        }
    }

    async fn maybe_enter_cool_down(&self, symbol: &str, now: DateTime<Utc>) -> Result<()> {
        let symbol_norm = symbol.to_uppercase();
        let active_remaining = self.tracker.count_active_setups(&symbol_norm).await?;
        if active_remaining > 0 {
            return Ok(());
        }
        let from = match self.tracker.get(&symbol_norm).await? {
            Some(r) => r.status,
            None => {
                warn!("maybe_enter_cool_down: {symbol_norm} vanished mid-flight");
                return Ok(());
            }
        };
        let cool_down_until = trading_days_after_close(now, COOL_DOWN_TRADING_DAYS);
        self.set_status_and_emit(
            &symbol_norm,
            from,
            TrackerStatus::CoolDown,
            None,
            Some(cool_down_until),
        )
        .await
    }

    async fn set_status_and_emit(
        &self,
        symbol: &str,
        from: TrackerStatus,
        to: TrackerStatus,
        in_play_until: Option<DateTime<Utc>>,
        cool_down_until: Option<DateTime<Utc>>,
    ) -> Result<()> {
        self.tracker
            .set_status(symbol, to, in_play_until, cool_down_until)
            .await?;
        if from != to {
            let _ = self
                .emitter
                .emit(AppEvent::TickerStatusChanged {
                    symbol: symbol.to_string(),
                    from,
                    to,
                })
                .await;
        }
        Ok(())
    }

    async fn update_source_meta(&self, symbol: &str, meta: serde_json::Value) -> Result<()> {
        let meta_json = serde_json::to_string(&meta)
            .map_err(|e| StateMachineError::Tracker(TrackerError::Serde(e)))?;
        let symbol_for_db = symbol.to_string();
        self.db
            .with_conn(move |conn| {
                conn.execute(
                    "UPDATE tracked_tickers SET source_meta = ?1 WHERE symbol = ?2",
                    rusqlite::params![meta_json, symbol_for_db],
                )?;
                Ok(())
            })
            .await?;
        Ok(())
    }
}
