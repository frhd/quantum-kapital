// allow-large-file: the setups CRUD block, the 24-column raw-row tuple,
// and the `decode_setup_raw` + `decode_sizing` helpers are tightly
// coupled to a single SELECT shape. Splitting the decode into a sibling
// file would force the row tuple to leak as a public type just to
// share it across files; keeping all of it in one place is the lower-
// surface-area choice. Quant-decisions Phase 1 added the sizing
// columns; further P4 grade columns are expected here too.
//! Setup-row CRUD methods for `TrackerService`.
//!
//! This file holds the `impl TrackerService { … }` block containing every
//! method that touches the `setups` table. Ticker-CRUD methods live in
//! `super` (`mod.rs`).

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use rusqlite::OptionalExtension;

use crate::ibkr::types::tracker::{Setup, SetupStatus};
use crate::services::risk_engine::{ConvictionGrade, Sizing, SizingSkippedReason};
use crate::storage::error::StorageError;
use crate::strategies::{Direction, SetupCandidate, SkipReason, TargetLevel};
use crate::utils::helpers::unix_to_utc;

use super::{Result, TrackerError, TrackerService};

impl TrackerService {
    /// Insert a new `Setup` row from a detector candidate. The caller
    /// passes the symbol explicitly because `SetupCandidate` does not
    /// carry it (the detector frame already knows the symbol from the
    /// surrounding `MarketContext`). Returns the persisted row with its
    /// generated `id` populated.
    pub async fn insert_setup(&self, symbol: &str, candidate: &SetupCandidate) -> Result<Setup> {
        let symbol_norm = symbol.to_uppercase();
        let strategy = candidate.strategy.to_string();
        let direction = candidate.direction;
        let direction_str = direction_as_str(direction).to_string();
        let detected_at = candidate.detected_at;
        let detected_at_unix = detected_at.timestamp();
        let trigger_price = candidate.trigger_price;
        let stop_price = candidate.stop_price;
        let targets = candidate.targets.clone();
        let targets_json = serde_json::to_string(&targets)?;
        let raw_signals = candidate.raw_signals.clone();
        let raw_signals_json = serde_json::to_string(&raw_signals)?;

        let symbol_for_db = symbol_norm.clone();
        let strategy_for_db = strategy.clone();

        let id = self
            .db
            .with_conn(move |conn| {
                conn.execute(
                    "INSERT INTO setups \
                     (symbol, strategy, direction, detected_at, trigger_price, stop_price, \
                      targets, raw_signals, thesis, thesis_json, status, invalidated_at, invalidation_reason) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL, NULL, 'active', NULL, NULL)",
                    rusqlite::params![
                        symbol_for_db,
                        strategy_for_db,
                        direction_str,
                        detected_at_unix,
                        trigger_price,
                        stop_price,
                        targets_json,
                        raw_signals_json,
                    ],
                )?;
                Ok(conn.last_insert_rowid())
            })
            .await?;

        Ok(Setup {
            id,
            symbol: symbol_norm,
            strategy,
            direction,
            detected_at,
            trigger_price,
            stop_price,
            targets,
            raw_signals,
            thesis: None,
            thesis_json: None,
            status: SetupStatus::Active,
            invalidated_at: None,
            invalidation_reason: None,
            archived_at: None,
            sizing: None,
            skipped_reason: None,
            skip_window_json: None,
        })
    }

    /// Phase 5 — persist a setup that the runner gated before sizing.
    /// `skipped_reason` is the short tag (e.g. `EarningsBlackout`);
    /// `skip_window_json` carries the full blackout descriptor so the
    /// UI can show "skipped: earnings in 3 BD" without a second query.
    /// The row lands with `status = 'active'` so it's still visible
    /// alongside fired setups; the runner just doesn't size it or
    /// drive the state machine. Risk-engine, alerts, thesis paths are
    /// all bypassed for skipped rows.
    pub async fn insert_skipped_setup(
        &self,
        symbol: &str,
        candidate: &SetupCandidate,
        skipped_reason: SkipReason,
        skip_window_json: serde_json::Value,
    ) -> Result<Setup> {
        let symbol_norm = symbol.to_uppercase();
        let strategy = candidate.strategy.to_string();
        let direction = candidate.direction;
        let direction_str = direction_as_str(direction).to_string();
        let detected_at = candidate.detected_at;
        let detected_at_unix = detected_at.timestamp();
        let trigger_price = candidate.trigger_price;
        let stop_price = candidate.stop_price;
        let targets = candidate.targets.clone();
        let targets_json = serde_json::to_string(&targets)?;
        let raw_signals = candidate.raw_signals.clone();
        let raw_signals_json = serde_json::to_string(&raw_signals)?;
        let skipped_reason_str = skipped_reason.as_str().to_string();
        let skip_window_str = serde_json::to_string(&skip_window_json)?;

        let symbol_for_db = symbol_norm.clone();
        let strategy_for_db = strategy.clone();

        let id = self
            .db
            .with_conn(move |conn| {
                conn.execute(
                    "INSERT INTO setups \
                     (symbol, strategy, direction, detected_at, trigger_price, stop_price, \
                      targets, raw_signals, thesis, thesis_json, status, invalidated_at, \
                      invalidation_reason, skipped_reason, skip_window_json) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL, NULL, 'active', NULL, NULL, ?9, ?10)",
                    rusqlite::params![
                        symbol_for_db,
                        strategy_for_db,
                        direction_str,
                        detected_at_unix,
                        trigger_price,
                        stop_price,
                        targets_json,
                        raw_signals_json,
                        skipped_reason_str,
                        skip_window_str,
                    ],
                )?;
                Ok(conn.last_insert_rowid())
            })
            .await?;

        Ok(Setup {
            id,
            symbol: symbol_norm,
            strategy,
            direction,
            detected_at,
            trigger_price,
            stop_price,
            targets,
            raw_signals,
            thesis: None,
            thesis_json: None,
            status: SetupStatus::Active,
            invalidated_at: None,
            invalidation_reason: None,
            archived_at: None,
            sizing: None,
            skipped_reason: Some(skipped_reason),
            skip_window_json: Some(skip_window_json),
        })
    }

    /// Phase 5 — list `setups` rows where `skipped_reason IS NOT NULL`,
    /// optionally filtered by trading day (rows with `detected_at >=
    /// since`). Newest-first. Used by the SkippedSetupsPanel to surface
    /// today's blackout-gated hits to the trader.
    pub async fn list_skipped_setups(&self, since: Option<DateTime<Utc>>) -> Result<Vec<Setup>> {
        let since_unix = since.map(|d| d.timestamp());
        let raws = self
            .db
            .with_conn(move |conn| {
                let mut sql = String::from(
                    "SELECT id, symbol, strategy, direction, detected_at, trigger_price, \
                            stop_price, targets, raw_signals, thesis, thesis_json, status, \
                            invalidated_at, invalidation_reason, archived_at, \
                            qty, dollar_risk_cents, r_per_share_cents, equity_at_decision_cents, \
                            sizing_version, sizing_skipped_reason, conviction_grade, \
                            conviction_multiplier_bps, sizing_cap_applied, \
                            skipped_reason, skip_window_json \
                     FROM setups \
                     WHERE archived_at IS NULL AND skipped_reason IS NOT NULL",
                );
                if since_unix.is_some() {
                    sql.push_str(" AND detected_at >= ?1");
                }
                sql.push_str(" ORDER BY detected_at DESC, id DESC");

                let mut stmt = conn.prepare(&sql)?;
                let rows = match since_unix {
                    Some(u) => stmt
                        .query_map(rusqlite::params![u], setup_row_to_raw)?
                        .collect::<rusqlite::Result<Vec<_>>>()?,
                    None => stmt
                        .query_map([], setup_row_to_raw)?
                        .collect::<rusqlite::Result<Vec<_>>>()?,
                };
                Ok(rows)
            })
            .await?;
        raws.into_iter().map(decode_setup_raw).collect()
    }

    /// Quant-decisions Phase 1 — persist the risk-engine `Sizing` for
    /// `id`. Called by `TrackerRunner` after `RiskEngine::size`
    /// returns; the row already exists from `insert_setup`. Returns
    /// the refreshed setup row, or `NotFound` if the id is unknown.
    pub async fn update_setup_sizing(&self, id: i64, sizing: &Sizing) -> Result<Setup> {
        let qty = sizing.qty as i64;
        let dollar_risk_cents = sizing.dollar_risk_cents;
        let r_per_share_cents = sizing.r_per_share_cents;
        let equity_at_decision_cents = sizing.equity_at_decision_cents;
        let sizing_version = sizing.version as i64;
        let conviction_grade = sizing.conviction_grade.as_str().to_string();
        let conviction_multiplier_bps = sizing.conviction_multiplier_bps as i64;
        let cap_applied = if sizing.cap_applied { 1_i64 } else { 0_i64 };
        let skipped_reason = sizing.skipped_reason.map(|r| r.as_str().to_string());

        let updated = self
            .db
            .with_conn(move |conn| {
                let n = conn.execute(
                    "UPDATE setups SET \
                       qty = ?1, \
                       dollar_risk_cents = ?2, \
                       r_per_share_cents = ?3, \
                       equity_at_decision_cents = ?4, \
                       sizing_version = ?5, \
                       sizing_skipped_reason = ?6, \
                       conviction_grade = ?7, \
                       conviction_multiplier_bps = ?8, \
                       sizing_cap_applied = ?9 \
                     WHERE id = ?10 AND archived_at IS NULL",
                    rusqlite::params![
                        qty,
                        dollar_risk_cents,
                        r_per_share_cents,
                        equity_at_decision_cents,
                        sizing_version,
                        skipped_reason,
                        conviction_grade,
                        conviction_multiplier_bps,
                        cap_applied,
                        id,
                    ],
                )?;
                Ok(n)
            })
            .await?;
        if updated == 0 {
            return Err(TrackerError::NotFound(format!("setup#{id}")));
        }
        self.get_setup(id)
            .await?
            .ok_or_else(|| TrackerError::NotFound(format!("setup#{id}")))
    }

    /// Phase 7 — persist the exit plan computed by the per-detector
    /// `ExitPolicy` at signal time. Stored alongside the version
    /// string so a post-hoc audit can tell which policy ran. Pre-P7
    /// rows have NULL in both columns; the `OrderTicket` falls back
    /// to the legacy static ladder for those.
    pub async fn update_setup_exit_plan(
        &self,
        id: i64,
        policy_version: String,
        exit_plan_json: serde_json::Value,
    ) -> Result<()> {
        let plan_str = serde_json::to_string(&exit_plan_json)?;
        let updated = self
            .db
            .with_conn(move |conn| {
                let n = conn.execute(
                    "UPDATE setups SET exit_policy_version = ?1, exit_plan_json = ?2 \
                     WHERE id = ?3 AND archived_at IS NULL",
                    rusqlite::params![policy_version, plan_str, id],
                )?;
                Ok(n)
            })
            .await?;
        if updated == 0 {
            return Err(TrackerError::NotFound(format!("setup#{id}")));
        }
        Ok(())
    }

    /// Phase 7 — read back the exit plan written by
    /// `update_setup_exit_plan`. Returns `None` for pre-P7 rows (the
    /// legacy static ladder applies); errors only on JSON-decode
    /// failure or storage error.
    pub async fn get_setup_exit_plan(
        &self,
        id: i64,
    ) -> Result<Option<crate::strategies::exits::ExitPlan>> {
        let raw: Option<(Option<String>, Option<String>)> = self
            .db
            .with_conn(move |conn| {
                conn.query_row(
                    "SELECT exit_policy_version, exit_plan_json FROM setups \
                     WHERE id = ?1 AND archived_at IS NULL",
                    rusqlite::params![id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()
                .map_err(StorageError::from)
            })
            .await?;
        let Some((_version, Some(plan_str))) = raw else {
            return Ok(None);
        };
        let plan = serde_json::from_str(&plan_str)?;
        Ok(Some(plan))
    }

    /// Persist a generated LLM thesis on a `setups` row. `thesis_md` is
    /// the human-readable markdown body; `thesis_json` is the full
    /// structured tool-output (conviction, invalidation_levels,
    /// risk_notes, …) stored as JSON. Returns the refreshed row, or
    /// `NotFound` if the id doesn't exist.
    pub async fn update_setup_thesis(
        &self,
        id: i64,
        thesis_md: String,
        thesis_json: serde_json::Value,
    ) -> Result<Setup> {
        let thesis_json_str = serde_json::to_string(&thesis_json)?;
        let updated = self
            .db
            .with_conn(move |conn| {
                let n = conn.execute(
                    "UPDATE setups SET thesis = ?1, thesis_json = ?2 \
                     WHERE id = ?3 AND archived_at IS NULL",
                    rusqlite::params![thesis_md, thesis_json_str, id],
                )?;
                Ok(n)
            })
            .await?;
        if updated == 0 {
            return Err(TrackerError::NotFound(format!("setup#{id}")));
        }
        self.get_setup(id)
            .await?
            .ok_or_else(|| TrackerError::NotFound(format!("setup#{id}")))
    }

    /// List setups, optionally filtered by `symbol` and / or by a
    /// `since` cutoff (rows with `detected_at >= since` only). Order is
    /// `detected_at DESC` so the freshest rows come first.
    pub async fn list_setups(
        &self,
        symbol: Option<&str>,
        since: Option<DateTime<Utc>>,
    ) -> Result<Vec<Setup>> {
        let symbol = symbol.map(|s| s.to_uppercase());
        let since_unix = since.map(|d| d.timestamp());
        let raws = self
            .db
            .with_conn(move |conn| {
                let mut sql = String::from(
                    "SELECT id, symbol, strategy, direction, detected_at, trigger_price, \
                            stop_price, targets, raw_signals, thesis, thesis_json, status, \
                            invalidated_at, invalidation_reason, archived_at, \
                            qty, dollar_risk_cents, r_per_share_cents, equity_at_decision_cents, \
                            sizing_version, sizing_skipped_reason, conviction_grade, \
                            conviction_multiplier_bps, sizing_cap_applied, \
                            skipped_reason, skip_window_json \
                     FROM setups WHERE archived_at IS NULL",
                );
                if symbol.is_some() {
                    sql.push_str(" AND symbol = ?1");
                }
                if since_unix.is_some() {
                    if symbol.is_some() {
                        sql.push_str(" AND detected_at >= ?2");
                    } else {
                        sql.push_str(" AND detected_at >= ?1");
                    }
                }
                sql.push_str(" ORDER BY detected_at DESC, id DESC");

                let mut stmt = conn.prepare(&sql)?;
                let rows = match (symbol.as_ref(), since_unix) {
                    (Some(s), Some(u)) => stmt
                        .query_map(rusqlite::params![s, u], setup_row_to_raw)?
                        .collect::<rusqlite::Result<Vec<_>>>()?,
                    (Some(s), None) => stmt
                        .query_map(rusqlite::params![s], setup_row_to_raw)?
                        .collect::<rusqlite::Result<Vec<_>>>()?,
                    (None, Some(u)) => stmt
                        .query_map(rusqlite::params![u], setup_row_to_raw)?
                        .collect::<rusqlite::Result<Vec<_>>>()?,
                    (None, None) => stmt
                        .query_map([], setup_row_to_raw)?
                        .collect::<rusqlite::Result<Vec<_>>>()?,
                };
                Ok(rows)
            })
            .await?;

        raws.into_iter().map(decode_setup_raw).collect()
    }

    #[allow(dead_code)]
    pub async fn get_setup(&self, id: i64) -> Result<Option<Setup>> {
        let raw = self
            .db
            .with_conn(move |conn| {
                conn.query_row(
                    "SELECT id, symbol, strategy, direction, detected_at, trigger_price, \
                            stop_price, targets, raw_signals, thesis, thesis_json, status, \
                            invalidated_at, invalidation_reason, archived_at, \
                            qty, dollar_risk_cents, r_per_share_cents, equity_at_decision_cents, \
                            sizing_version, sizing_skipped_reason, conviction_grade, \
                            conviction_multiplier_bps, sizing_cap_applied, \
                            skipped_reason, skip_window_json \
                     FROM setups WHERE id = ?1 AND archived_at IS NULL",
                    rusqlite::params![id],
                    setup_row_to_raw,
                )
                .optional()
                .map_err(StorageError::from)
            })
            .await?;
        match raw {
            Some(r) => Ok(Some(decode_setup_raw(r)?)),
            None => Ok(None),
        }
    }

    /// Look up the most recent `setups` row for `(symbol, strategy,
    /// direction)` whose `detected_at` falls within the last
    /// `within` window. Returns the row's `id` if a match exists, or
    /// `None` if there is no recent duplicate. Used by the runner to
    /// short-circuit re-emitting the same signal twice in a single
    /// trading day.
    pub async fn recent_duplicate(
        &self,
        symbol: &str,
        strategy: &str,
        direction: Direction,
        within: ChronoDuration,
    ) -> Result<Option<i64>> {
        let symbol = symbol.to_uppercase();
        let strategy = strategy.to_string();
        let direction_str = direction_as_str(direction).to_string();
        let cutoff_unix = (Utc::now() - within).timestamp();
        let id = self
            .db
            .with_conn(move |conn| {
                conn.query_row(
                    "SELECT id FROM setups \
                     WHERE symbol = ?1 AND strategy = ?2 AND direction = ?3 \
                       AND detected_at >= ?4 AND archived_at IS NULL \
                     ORDER BY detected_at DESC LIMIT 1",
                    rusqlite::params![symbol, strategy, direction_str, cutoff_unix],
                    |row| row.get::<_, i64>(0),
                )
                .optional()
                .map_err(StorageError::from)
            })
            .await?;
        Ok(id)
    }

    /// Count `Active` setups for `symbol`. Used by the state machine to
    /// decide whether invalidating one setup should flip the ticker to
    /// `CoolDown` (only when no other active setups remain).
    #[allow(dead_code)]
    pub async fn count_active_setups(&self, symbol: &str) -> Result<usize> {
        let symbol = symbol.to_uppercase();
        let count = self
            .db
            .with_conn(move |conn| {
                let n: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM setups \
                     WHERE symbol = ?1 AND status = 'active' AND archived_at IS NULL",
                    rusqlite::params![symbol],
                    |row| row.get(0),
                )?;
                Ok(n)
            })
            .await?;
        Ok(count.max(0) as usize)
    }

    /// Update a `setups` row's lifecycle status. When transitioning to
    /// `Invalidated`, callers pass the reason and the timestamp; for
    /// `Completed`, only the timestamp. Returns the persisted row, or
    /// `NotFound` if the id doesn't exist.
    #[allow(dead_code)]
    pub async fn update_setup_status(
        &self,
        id: i64,
        status: SetupStatus,
        reason: Option<String>,
        invalidated_at: Option<DateTime<Utc>>,
    ) -> Result<Setup> {
        let status_str = status.as_str().to_string();
        let reason_for_db = reason.clone();
        let invalidated_unix = invalidated_at.map(|d| d.timestamp());
        let updated = self
            .db
            .with_conn(move |conn| {
                let n = conn.execute(
                    "UPDATE setups SET status = ?1, invalidation_reason = ?2, invalidated_at = ?3 \
                     WHERE id = ?4 AND archived_at IS NULL",
                    rusqlite::params![status_str, reason_for_db, invalidated_unix, id],
                )?;
                Ok(n)
            })
            .await?;
        if updated == 0 {
            return Err(TrackerError::NotFound(format!("setup#{id}")));
        }
        self.get_setup(id)
            .await?
            .ok_or_else(|| TrackerError::NotFound(format!("setup#{id}")))
    }
}

// ---------------- setup row helpers ----------------

// allow-large-tuple: 26 fields parallel the `setups` SELECT order 1:1
// so the row reader stays a single positional decode. Splitting would
// require a struct shim that adds nothing the comment doesn't. Phase 5
// appends `skipped_reason` + `skip_window_json` at the tail.
#[allow(clippy::type_complexity)]
type SetupRawRow = (
    i64,            // id
    String,         // symbol
    String,         // strategy
    String,         // direction
    i64,            // detected_at unix
    f64,            // trigger_price
    f64,            // stop_price
    String,         // targets json
    String,         // raw_signals json
    Option<String>, // thesis (markdown)
    Option<String>, // thesis_json (full structured)
    String,         // status
    Option<i64>,    // invalidated_at unix
    Option<String>, // invalidation_reason
    Option<i64>,    // archived_at unix
    Option<i64>,    // qty
    Option<i64>,    // dollar_risk_cents
    Option<i64>,    // r_per_share_cents
    Option<i64>,    // equity_at_decision_cents
    Option<i64>,    // sizing_version
    Option<String>, // sizing_skipped_reason
    Option<String>, // conviction_grade
    Option<i64>,    // conviction_multiplier_bps
    Option<i64>,    // sizing_cap_applied
    Option<String>, // skipped_reason (Phase 5)
    Option<String>, // skip_window_json (Phase 5)
);

fn setup_row_to_raw(row: &rusqlite::Row<'_>) -> rusqlite::Result<SetupRawRow> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
        row.get(9)?,
        row.get(10)?,
        row.get(11)?,
        row.get(12)?,
        row.get(13)?,
        row.get(14)?,
        row.get(15)?,
        row.get(16)?,
        row.get(17)?,
        row.get(18)?,
        row.get(19)?,
        row.get(20)?,
        row.get(21)?,
        row.get(22)?,
        row.get(23)?,
        row.get(24)?,
        row.get(25)?,
    ))
}

fn decode_setup_raw(r: SetupRawRow) -> Result<Setup> {
    let (
        id,
        symbol,
        strategy,
        direction_s,
        detected_at,
        trigger_price,
        stop_price,
        targets_s,
        raw_signals_s,
        thesis,
        thesis_json_s,
        status_s,
        invalidated_at,
        invalidation_reason,
        archived_at,
        qty,
        dollar_risk_cents,
        r_per_share_cents,
        equity_at_decision_cents,
        sizing_version,
        sizing_skipped_reason,
        conviction_grade_s,
        conviction_multiplier_bps,
        sizing_cap_applied,
        skipped_reason_s,
        skip_window_json_s,
    ) = r;
    let direction = parse_direction(&direction_s).ok_or_else(|| {
        TrackerError::Storage(StorageError::Migration(format!(
            "unknown direction '{direction_s}' on setup {id}"
        )))
    })?;
    let status = SetupStatus::parse(&status_s).ok_or_else(|| {
        TrackerError::Storage(StorageError::Migration(format!(
            "unknown setup status '{status_s}' on setup {id}"
        )))
    })?;
    let targets: Vec<TargetLevel> = serde_json::from_str(&targets_s)?;
    let raw_signals: serde_json::Value = serde_json::from_str(&raw_signals_s)?;
    let thesis_json = match thesis_json_s {
        Some(s) if !s.is_empty() => Some(serde_json::from_str::<serde_json::Value>(&s)?),
        _ => None,
    };
    let sizing = decode_sizing(
        id,
        qty,
        dollar_risk_cents,
        r_per_share_cents,
        equity_at_decision_cents,
        sizing_version,
        sizing_skipped_reason,
        conviction_grade_s,
        conviction_multiplier_bps,
        sizing_cap_applied,
    )?;
    let skipped_reason = match skipped_reason_s.as_deref() {
        Some(s) => Some(SkipReason::parse(s).ok_or_else(|| {
            TrackerError::Storage(StorageError::Migration(format!(
                "unknown skipped_reason '{s}' on setup {id}"
            )))
        })?),
        None => None,
    };
    let skip_window_json = match skip_window_json_s {
        Some(s) if !s.is_empty() => Some(serde_json::from_str::<serde_json::Value>(&s)?),
        _ => None,
    };
    Ok(Setup {
        id,
        symbol,
        strategy,
        direction,
        detected_at: unix_to_utc(detected_at),
        trigger_price,
        stop_price,
        targets,
        raw_signals,
        thesis,
        thesis_json,
        status,
        invalidated_at: invalidated_at.map(unix_to_utc),
        invalidation_reason,
        archived_at: archived_at.map(unix_to_utc),
        sizing,
        skipped_reason,
        skip_window_json,
    })
}

#[allow(clippy::too_many_arguments)]
fn decode_sizing(
    id: i64,
    qty: Option<i64>,
    dollar_risk_cents: Option<i64>,
    r_per_share_cents: Option<i64>,
    equity_at_decision_cents: Option<i64>,
    sizing_version: Option<i64>,
    skipped_reason_s: Option<String>,
    conviction_grade_s: Option<String>,
    conviction_multiplier_bps: Option<i64>,
    cap_applied: Option<i64>,
) -> Result<Option<Sizing>> {
    // Pre-P1 rows have NULL sizing_version. Treat those as "ungated"
    // and surface as None to the UI.
    let version = match sizing_version {
        Some(v) => v as i32,
        None => return Ok(None),
    };
    let grade = conviction_grade_s
        .as_deref()
        .and_then(ConvictionGrade::parse)
        .unwrap_or(ConvictionGrade::C);
    let skipped_reason = match skipped_reason_s.as_deref() {
        Some(s) => Some(SizingSkippedReason::parse(s).ok_or_else(|| {
            TrackerError::Storage(StorageError::Migration(format!(
                "unknown sizing_skipped_reason '{s}' on setup {id}"
            )))
        })?),
        None => None,
    };
    Ok(Some(Sizing {
        qty: qty.unwrap_or(0).max(0) as u32,
        dollar_risk_cents: dollar_risk_cents.unwrap_or(0),
        r_per_share_cents: r_per_share_cents.unwrap_or(0),
        equity_at_decision_cents: equity_at_decision_cents.unwrap_or(0),
        conviction_grade: grade,
        conviction_multiplier_bps: conviction_multiplier_bps.unwrap_or(0).max(0) as u32,
        cap_applied: cap_applied.unwrap_or(0) != 0,
        skipped_reason,
        version,
    }))
}

fn direction_as_str(direction: Direction) -> &'static str {
    match direction {
        Direction::Long => "long",
        Direction::Short => "short",
    }
}

fn parse_direction(s: &str) -> Option<Direction> {
    match s {
        "long" => Some(Direction::Long),
        "short" => Some(Direction::Short),
        _ => None,
    }
}
