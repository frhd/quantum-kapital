//! Phase 11 — `services/tilt_guard/`: account-level circuit breaker.
//!
//! Master invariant: prevent the third revenge trade after a -3R day or
//! two consecutive losing closed trades. The service is a **gate, not
//! an actor** — it never places close-out orders, never modifies
//! existing brackets. It refuses *new* setup placement until the
//! configured auto-reset (next session open) or a logged manual
//! override.
//!
//! Three surfaces:
//!
//!   - [`TiltGuardService::evaluate`] — pull today's R-stream, walk the
//!     triggers, open a `tilt_episodes` row and emit `TiltActivated`
//!     when a rule fires. Idempotent on already-paused state.
//!   - [`TiltGuardService::status`] — read-only "are we paused right
//!     now?" view used by the UI banner + the upstream gates
//!     (`RiskEngine`, `OrderTicket`).
//!   - [`TiltGuardService::override_pause`] — close the open episode
//!     with a logged reason; mirrored to `gate_overrides`.
//!
//! Trigger evaluation cadence: lazy at sizing time + on demand
//! (`tilt_guard_status` / `tilt_guard_override` Tauri commands).
//! Master committed "evaluate on every BracketStatusChanged event when
//! status reaches a terminal state"; the production fill-status
//! reconciler isn't shipped yet (P3 QUESTIONS.md), so the lazy path is
//! the load-bearing one. Logged in QUESTIONS.md so a future maintainer
//! can layer a dedicated subscriber when the reconciler lands.
//!
//! Day-N+1 stricter threshold: a `manual_override` release on day N
//! lifts day N+1's cumulative-R floor by 1R (so -2R triggers tilt the
//! next day). State derived from the previous trading day's
//! `tilt_episodes.release_kind` — no extra column needed.

#![allow(dead_code)]

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Duration as ChronoDuration, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::{Mutex, RwLock};
use tracing::{info, warn};

use crate::events::{AppEvent, EventEmitter};
use crate::ibkr::error::IbkrError;
use crate::mcp::tools::executions::ExecutionRow;
use crate::services::risk_engine::AccountSource;
use crate::services::trade_legs::match_legs;
use crate::storage::error::StorageError;
use crate::storage::Db;
use crate::utils::market_calendar::{et_date, next_open_at, trading_days_before};

mod state;
mod triggers;

#[cfg(test)]
mod tests;

pub use state::{NewTiltEpisode, ReleaseKind, TiltEpisode, TiltEpisodeStore};
pub use triggers::{effective_cum_r_threshold, evaluate_triggers, ClosedTrade};
#[allow(unused_imports)]
pub use triggers::{TriggerEval, TriggerKind};

/// Tunable knobs. Defaults match master `Defaults committed`:
/// `cum_r_threshold = -3.0`, `consecutive_loss_threshold = 2`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TiltConfig {
    pub cum_r_threshold: f64,
    pub consecutive_loss_threshold: u32,
}

impl Default for TiltConfig {
    fn default() -> Self {
        Self {
            cum_r_threshold: -3.0,
            consecutive_loss_threshold: 2,
        }
    }
}

/// Trait seam over "give me today's closed trades for this account."
/// Production: `DbClosedTradeSource` joins `executions` against
/// `setups.dollar_risk_cents` to compute realized R per FIFO leg.
/// Tests: a canned `Vec<ClosedTrade>` so triggers can be exercised
/// without a fills schema.
#[async_trait]
pub trait ClosedTradeSource: Send + Sync {
    async fn closed_trades_today(
        &self,
        account: &str,
        et_date: NaiveDate,
    ) -> std::result::Result<Vec<ClosedTrade>, TiltError>;
}

#[derive(Error, Debug)]
pub enum TiltError {
    #[error("storage: {0}")]
    Storage(#[from] StorageError),
    #[error("ibkr: {0}")]
    Ibkr(#[from] IbkrError),
    #[error("emitter: {0}")]
    Emitter(String),
}

pub type Result<T> = std::result::Result<T, TiltError>;

/// Snapshot of "are we paused?". Returned by the Tauri command and by
/// the upstream gates so the modal can grey out Send before sizing
/// even runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TiltStatus {
    pub account: String,
    pub paused: bool,
    /// The open episode when `paused == true`; the most recent
    /// released episode otherwise (UI shows "released at ..." copy).
    pub episode: Option<TiltEpisodeView>,
    /// Effective cumulative-R floor for the current ET day. -3.0 by
    /// default; -2.0 after a manual override the previous trading day.
    pub day_threshold_cum_r: f64,
    /// Cumulative R observed *so far* today; rendered as a subtitle on
    /// the banner/card.
    pub cumulative_r_today: f64,
    /// Number of closed trades counted into `cumulative_r_today`.
    pub closed_trade_count_today: usize,
}

/// JSON-friendly projection of a `TiltEpisode`. Keeps the wire shape
/// stable even if the storage row grows new columns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TiltEpisodeView {
    pub id: i64,
    pub account: String,
    pub triggered_at: DateTime<Utc>,
    pub trigger_kind: String,
    pub cumulative_r: f64,
    pub consecutive_losses: u32,
    pub auto_reset_at: DateTime<Utc>,
    pub released_at: Option<DateTime<Utc>>,
    pub release_kind: Option<String>,
    pub release_reason: Option<String>,
}

impl From<&TiltEpisode> for TiltEpisodeView {
    fn from(ep: &TiltEpisode) -> Self {
        Self {
            id: ep.id,
            account: ep.account.clone(),
            triggered_at: ep.triggered_at,
            trigger_kind: ep.trigger_kind.as_str().to_string(),
            cumulative_r: ep.cumulative_r_milli as f64 / 1000.0,
            consecutive_losses: ep.consecutive_losses,
            auto_reset_at: ep.auto_reset_at,
            released_at: ep.released_at,
            release_kind: ep.release_kind.map(|k| k.as_str().to_string()),
            release_reason: ep.release_reason.clone(),
        }
    }
}

/// The service. Cheap to clone (`Arc` internals) so it can be
/// `app.manage`'d once and pulled as `State<Arc<TiltGuardService>>`.
#[derive(Clone)]
pub struct TiltGuardService {
    store: Arc<TiltEpisodeStore>,
    closed_trades: Arc<dyn ClosedTradeSource>,
    account_source: Arc<dyn AccountSource>,
    emitter: Arc<EventEmitter>,
    config: Arc<RwLock<TiltConfig>>,
    /// Single-flight guard so concurrent triggers (sizing + status +
    /// override) collapse onto one `evaluate` round-trip rather than
    /// racing on the open-episode read/write.
    eval_lock: Arc<Mutex<()>>,
    db: Arc<Db>,
}

impl TiltGuardService {
    pub fn new(
        db: Arc<Db>,
        store: Arc<TiltEpisodeStore>,
        closed_trades: Arc<dyn ClosedTradeSource>,
        account_source: Arc<dyn AccountSource>,
        emitter: Arc<EventEmitter>,
        config: TiltConfig,
    ) -> Self {
        Self {
            store,
            closed_trades,
            account_source,
            emitter,
            config: Arc::new(RwLock::new(config)),
            eval_lock: Arc::new(Mutex::new(())),
            db,
        }
    }

    /// Resolve the active account through the configured source.
    /// Mirrors `RiskEngine`'s policy (first IBKR account at runtime).
    pub async fn current_account(&self) -> Result<String> {
        Ok(self.account_source.current_account().await?)
    }

    pub async fn config(&self) -> TiltConfig {
        self.config.read().await.clone()
    }

    pub async fn set_config(&self, cfg: TiltConfig) {
        *self.config.write().await = cfg;
    }

    /// True when the account currently has an open tilt episode (and
    /// the auto-reset hasn't elapsed). Cheap; a single SQL read.
    pub async fn is_paused(&self, account: &str) -> Result<bool> {
        Ok(self.status(account).await?.paused)
    }

    /// Read-only "where are we right now" snapshot. Auto-releases an
    /// open episode whose `auto_reset_at` has elapsed. Does NOT
    /// re-evaluate triggers — for that, call [`Self::evaluate`].
    pub async fn status(&self, account: &str) -> Result<TiltStatus> {
        let _guard = self.eval_lock.lock().await;
        self.status_inner(account).await
    }

    async fn status_inner(&self, account: &str) -> Result<TiltStatus> {
        let now = Utc::now();
        let mut open = self.store.open_for_account(account).await?;
        if let Some(ep) = open.as_ref() {
            if ep.auto_reset_at <= now {
                let flipped = self
                    .store
                    .release(ep.id, ReleaseKind::Auto, None, now)
                    .await?;
                if flipped {
                    let _ = self
                        .emitter
                        .emit(AppEvent::TiltReleased {
                            episode_id: ep.id,
                            account: account.to_string(),
                            release_kind: ReleaseKind::Auto.as_str().to_string(),
                        })
                        .await;
                }
                open = None;
            }
        }
        let day_threshold = self.day_threshold_for_account(account, now).await?;
        let trades = self
            .closed_trades
            .closed_trades_today(account, et_date(now))
            .await?;
        let cum: f64 = trades
            .iter()
            .map(|t| {
                if t.realized_r.is_finite() {
                    t.realized_r
                } else {
                    0.0
                }
            })
            .sum();
        let count = trades.len();

        let last_released = match open.as_ref() {
            Some(_) => None,
            None => self
                .store
                .history_since(account, now - ChronoDuration::days(7))
                .await?
                .into_iter()
                .next(),
        };
        let episode_view = open
            .as_ref()
            .map(TiltEpisodeView::from)
            .or_else(|| last_released.as_ref().map(TiltEpisodeView::from));

        Ok(TiltStatus {
            account: account.to_string(),
            paused: open.is_some(),
            episode: episode_view,
            day_threshold_cum_r: day_threshold,
            cumulative_r_today: cum,
            closed_trade_count_today: count,
        })
    }

    /// Lazy trigger evaluation: pulls today's R-stream, walks
    /// triggers, opens a new `tilt_episodes` row + emits
    /// `TiltActivated` when a rule fires. Auto-releases an elapsed
    /// open episode first (same logic as `status`). Returns the
    /// post-evaluation status.
    pub async fn evaluate(&self, account: &str) -> Result<TiltStatus> {
        let _guard = self.eval_lock.lock().await;
        let now = Utc::now();

        // Auto-release stale open episode first (before evaluating new
        // triggers — otherwise a Friday tilt would prevent a Monday
        // open from triggering its own).
        if let Some(ep) = self.store.open_for_account(account).await? {
            if ep.auto_reset_at <= now {
                let flipped = self
                    .store
                    .release(ep.id, ReleaseKind::Auto, None, now)
                    .await?;
                if flipped {
                    let _ = self
                        .emitter
                        .emit(AppEvent::TiltReleased {
                            episode_id: ep.id,
                            account: account.to_string(),
                            release_kind: ReleaseKind::Auto.as_str().to_string(),
                        })
                        .await;
                }
            } else {
                // Already paused; no-op.
                return self.status_inner(account).await;
            }
        }

        let day_threshold = self.day_threshold_for_account(account, now).await?;
        let cfg = self.config.read().await.clone();
        let mut trades = self
            .closed_trades
            .closed_trades_today(account, et_date(now))
            .await?;
        // Watermark: any trade closed at or before the most recent
        // release timestamp was already accounted for by the
        // override / auto-release. Filtering preserves master's
        // "override is a session acknowledgment" intent — the next
        // pause must come from genuinely new closed trades.
        if let Some(watermark) = self.store.last_released_at(account).await? {
            trades.retain(|t| t.closed_at > watermark);
        }
        let trigger = evaluate_triggers(&trades, day_threshold, cfg.consecutive_loss_threshold);

        if let Some(ev) = trigger {
            // Activation time = when the system noticed the breach
            // (`now`), not the trade's `closed_at`. Two reasons: (a)
            // a tilt rule that fires in the morning against trades
            // that closed yesterday must auto-reset on *tomorrow's*
            // open, not yesterday's open; (b) the audit row's
            // "triggered_at" should match what the trader experienced,
            // which is the moment the banner appeared.
            let auto_reset = next_open_at(now);
            let new_row = NewTiltEpisode {
                account: account.to_string(),
                triggered_at: now,
                trigger_kind: ev.kind,
                cumulative_r_milli: (ev.cumulative_r * 1000.0).round() as i64,
                consecutive_losses: ev.consecutive_losses,
                auto_reset_at: auto_reset,
            };
            let inserted = self.store.insert(new_row).await?;
            info!(
                account = %account,
                kind = %ev.kind.as_str(),
                cum_r = ev.cumulative_r,
                consec = ev.consecutive_losses,
                "tilt_guard: activated"
            );
            let _ = self
                .emitter
                .emit(AppEvent::TiltActivated {
                    episode_id: inserted.id,
                    account: account.to_string(),
                    trigger_kind: ev.kind.as_str().to_string(),
                    cumulative_r: ev.cumulative_r,
                    auto_reset_at: auto_reset,
                })
                .await;
        }

        self.status_inner(account).await
    }

    /// Close the open episode with a logged reason. Mirrors into
    /// `gate_overrides` with `gate_kind = 'tilt'` so the unified
    /// override audit (master cross-phase verification) sees it. No-op
    /// when the account isn't paused.
    pub async fn override_pause(&self, account: &str, reason: String) -> Result<TiltStatus> {
        if reason.trim().is_empty() {
            warn!(
                account = %account,
                "tilt_guard: override rejected — empty reason"
            );
        }
        let _guard = self.eval_lock.lock().await;
        let now = Utc::now();
        let open = match self.store.open_for_account(account).await? {
            Some(ep) => ep,
            None => return self.status_inner(account).await,
        };
        let flipped = self
            .store
            .release(
                open.id,
                ReleaseKind::ManualOverride,
                Some(reason.clone()),
                now,
            )
            .await?;
        if flipped {
            self.write_gate_override(account, &reason, now).await?;
            let _ = self
                .emitter
                .emit(AppEvent::TiltReleased {
                    episode_id: open.id,
                    account: account.to_string(),
                    release_kind: ReleaseKind::ManualOverride.as_str().to_string(),
                })
                .await;
            info!(
                account = %account,
                episode = open.id,
                "tilt_guard: manual override"
            );
        }
        self.status_inner(account).await
    }

    /// History — most-recent first. `since` cutoff lets the trader
    /// profile rollup query "this month's tilts".
    pub async fn history(
        &self,
        account: &str,
        since: DateTime<Utc>,
    ) -> Result<Vec<TiltEpisodeView>> {
        let rows = self.store.history_since(account, since).await?;
        Ok(rows.iter().map(TiltEpisodeView::from).collect())
    }

    async fn day_threshold_for_account(&self, account: &str, now: DateTime<Utc>) -> Result<f64> {
        let cfg = self.config.read().await.clone();
        let prev_day = trading_days_before(et_date(now), 1);
        let prev_release = self
            .store
            .last_release_on_et_date(account, prev_day)
            .await?;
        let prev_overridden = prev_release
            .as_ref()
            .map(|ep| ep.release_kind == Some(ReleaseKind::ManualOverride))
            .unwrap_or(false);
        Ok(effective_cum_r_threshold(
            cfg.cum_r_threshold,
            prev_overridden,
        ))
    }

    /// Insert a `gate_overrides` row with `gate_kind = 'tilt'`. Tilt
    /// overrides are account-level (not setup-level), so we pick the
    /// most recent open setup id we can find as a placeholder — the
    /// table requires `setup_id NOT NULL`. When no setup exists yet
    /// (fresh install / pre-trading), the audit insert is skipped
    /// rather than failing the override flow.
    async fn write_gate_override(
        &self,
        account: &str,
        reason: &str,
        at: DateTime<Utc>,
    ) -> Result<()> {
        let account_owned = account.to_string();
        let reason_owned = reason.to_string();
        let at_unix = at.timestamp();
        self.db
            .with_conn(move |conn| {
                let setup_id: Option<i64> = conn
                    .query_row(
                        "SELECT s.id FROM setups s
                         JOIN bracket_groups b ON b.setup_id = s.id
                         WHERE b.account = ?1
                         ORDER BY b.placed_at DESC
                         LIMIT 1",
                        rusqlite::params![account_owned],
                        |row| row.get(0),
                    )
                    .ok();
                let Some(setup_id) = setup_id else {
                    return Ok(());
                };
                conn.execute(
                    "INSERT INTO gate_overrides (
                        setup_id, gate_kind, reason, actor, at_unix
                     ) VALUES (?1, 'tilt', ?2, 'human', ?3)",
                    rusqlite::params![setup_id, reason_owned, at_unix],
                )?;
                Ok(())
            })
            .await?;
        Ok(())
    }
}

/// Production [`ClosedTradeSource`] — reads `executions` for the day,
/// runs the FIFO matcher, and joins each leg's `setup_id` against
/// `setups.dollar_risk_cents` to compute realized R. Mirrors the math
/// in `services/trade_reviews/scoring.rs::compute_v2_fields` so a
/// tilt-day's R-stream and that day's review row see identical R per
/// leg.
#[derive(Clone)]
pub struct DbClosedTradeSource {
    db: Arc<Db>,
}

impl DbClosedTradeSource {
    pub fn new(db: Arc<Db>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl ClosedTradeSource for DbClosedTradeSource {
    async fn closed_trades_today(
        &self,
        account: &str,
        et_date: NaiveDate,
    ) -> Result<Vec<ClosedTrade>> {
        let rows = fetch_executions_with_linkage(&self.db, account, et_date).await?;
        let legs = match_legs(&rows);
        let setup_ids: Vec<i64> = legs.iter().filter_map(|l| l.setup_id).collect();
        let dollar_risk_by_setup = fetch_dollar_risk_for_setups(&self.db, &setup_ids).await?;

        let mut out: Vec<ClosedTrade> = Vec::new();
        for leg in legs {
            // Master gotcha: only closed legs count. `match_legs`
            // emits `closed_at = Some(_)` for legs whose buy/sell qty
            // pair off; carryover legs (still open) get NULL.
            let Some(closed_at) = leg.closed_at else {
                continue;
            };
            let Some(sid) = leg.setup_id else { continue };
            let dollar_risk = match dollar_risk_by_setup
                .iter()
                .find(|(id, _)| *id == sid)
                .and_then(|(_, dr)| *dr)
            {
                Some(dr) if dr > 1e-9 => dr,
                _ => continue,
            };
            let realized_r = leg.net_pnl / dollar_risk;
            out.push(ClosedTrade {
                closed_at,
                realized_r,
            });
        }
        out.sort_by_key(|t| t.closed_at);
        Ok(out)
    }
}

async fn fetch_executions_with_linkage(
    db: &Arc<Db>,
    account: &str,
    et_date: NaiveDate,
) -> std::result::Result<Vec<ExecutionRow>, StorageError> {
    use crate::services::executions::ExecutionsStore;
    let store = ExecutionsStore::new(Arc::clone(db));
    store.query_with_linkage(account, et_date).await
}

async fn fetch_dollar_risk_for_setups(
    db: &Arc<Db>,
    setup_ids: &[i64],
) -> std::result::Result<Vec<(i64, Option<f64>)>, StorageError> {
    if setup_ids.is_empty() {
        return Ok(Vec::new());
    }
    let ids: Vec<i64> = setup_ids
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    db.with_conn(move |conn| {
        let mut stmt = conn.prepare("SELECT id, dollar_risk_cents FROM setups WHERE id = ?1")?;
        let mut out: Vec<(i64, Option<f64>)> = Vec::with_capacity(ids.len());
        for id in ids {
            if let Ok((sid, cents)) = stmt.query_row(rusqlite::params![id], |row| {
                let sid: i64 = row.get(0)?;
                let cents: Option<i64> = row.get(1)?;
                Ok((sid, cents))
            }) {
                out.push((sid, cents.map(|c| c as f64 / 100.0)));
            }
        }
        Ok(out)
    })
    .await
}
