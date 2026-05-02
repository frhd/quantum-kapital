//! [`AvCallLedger`] — Phase 5 AV-side guardrail.
//!
//! The composite provider's AV branch consults the ledger before
//! consulting the [`super::alpha_vantage::AlphaVantageFundamentalsProvider`].
//! The ledger enforces three caps per local-timezone day:
//!
//! 1. **Per-symbol cap** (default `1`) — protects against a noisy UI
//!    or a stuck loop hammering the same ticker.
//! 2. **Daily soft cap** (default `20`) — past this, every consult emits
//!    a `warn!`. The call still goes through; soft cap is a trip-wire.
//! 3. **Daily hard cap** (default `25`) — refuses the call. Mirrors the
//!    AV free-tier daily quota documented at
//!    <https://www.alphavantage.co/support/#api-key>.
//!
//! State is kept in two places:
//!
//! * In-memory `Mutex<…>` maps for the hot path (~microseconds per
//!   check).
//! * SQLite `av_call_ledger` + `av_per_symbol_ledger` tables (V10) so a
//!   process restart cannot reset the count and silently double-up
//!   against the daily quota.
//!
//! Only the AV branch of the composite consults the ledger. The manual
//! store (Hard Invariant #8) bypasses it; AV cache hits also bypass it
//! (the `is_cache_fresh` short-circuit lives in
//! [`super::composite::CompositeFundamentalsProvider`]). See
//! `loop/plan/phase-5-cutover.md` § "Decisions to make in this phase".

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::{Local, NaiveDate};
use thiserror::Error;
use tracing::warn;

use crate::storage::error::StorageError;
use crate::storage::Db;

/// Default daily soft cap. Past this, the ledger emits a `warn!` per
/// consult so operators can see the trip-wire fire.
pub const DEFAULT_SOFT_CAP: u32 = 20;

/// Default daily hard cap. Mirrors AV free tier (25 req/day).
pub const DEFAULT_HARD_CAP: u32 = 25;

/// Default per-symbol per-day cap.
pub const DEFAULT_PER_SYMBOL_CAP: u32 = 1;

/// Outcome of a successful [`AvCallLedger::reserve`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReserveOutcome {
    /// Reservation acquired below the soft cap. No warning emitted.
    BelowSoftCap,
    /// Reservation acquired between the soft and hard caps. The
    /// composite branch emits a warn-log so the operator can see the
    /// trip-wire firing.
    AboveSoftCap,
}

/// Error variants returned by [`AvCallLedger::reserve`]. Both the
/// composite provider and any future caller pattern-match on these to
/// decide whether to fall through to a stale-cache read or surface a
/// typed [`super::FundamentalsError`].
#[derive(Debug, Error)]
pub enum AvLedgerError {
    /// Daily hard cap reached. `hit_count` is the count seen at the
    /// moment the cap tripped (typically equal to the configured hard
    /// cap; carried so the surfacing layer can render a precise
    /// message).
    #[error("Alpha Vantage daily call ledger hard cap reached ({hit_count} calls today)")]
    DailyCapReached { hit_count: u32 },

    /// Per-symbol per-day cap reached.
    #[error("Alpha Vantage per-symbol cap reached for {symbol} ({count} call(s) today)")]
    PerSymbolCapReached { symbol: String, count: u32 },

    /// SQLite or pool error while persisting the ledger row. The
    /// composite layer treats this as a critical failure (we don't
    /// silently let an AV call slip past the cap on disk error).
    #[error("AV ledger persistence failed: {0}")]
    Storage(#[from] StorageError),
}

/// Local-timezone date supplier. Production wires the wall clock; tests
/// inject a fixed-date supplier so daily-rollover behaviour is
/// deterministic. Returns owned `NaiveDate` values to avoid lifetime
/// bleed into the ledger's `Mutex`-guarded maps.
pub trait DateSource: Send + Sync + 'static {
    fn today(&self) -> NaiveDate;
}

/// Production `DateSource`: wall-clock local-timezone date.
pub struct LocalDateSource;

impl DateSource for LocalDateSource {
    fn today(&self) -> NaiveDate {
        Local::now().date_naive()
    }
}

/// In-memory + SQLite-backed Alpha Vantage call ledger.
///
/// Lock discipline: the daily-counter mutex and per-symbol-counter
/// mutex are taken sequentially (never together) so deadlock is
/// impossible. SQLite writes happen outside the mutex via
/// [`Db::with_conn`]; the in-memory counters are the source of truth
/// during a single process run, with SQLite serving as a
/// crash-survival mirror.
pub struct AvCallLedger {
    db: Arc<Db>,
    soft_cap: u32,
    hard_cap: u32,
    per_symbol_cap: u32,
    daily_counts: Mutex<HashMap<NaiveDate, u32>>,
    per_symbol_counts: Mutex<HashMap<(NaiveDate, String), u32>>,
    date_source: Arc<dyn DateSource>,
}

impl AvCallLedger {
    /// Build a ledger with default caps (soft 20, hard 25, per-symbol 1)
    /// and the wall-clock date source. The in-memory counters are
    /// hydrated lazily from SQLite on the first [`Self::reserve`] call
    /// so the constructor stays sync.
    pub fn new(db: Arc<Db>) -> Self {
        Self::with_caps(
            db,
            DEFAULT_SOFT_CAP,
            DEFAULT_HARD_CAP,
            DEFAULT_PER_SYMBOL_CAP,
            Arc::new(LocalDateSource),
        )
    }

    /// Build a ledger with custom caps. Tests use this to drive
    /// soft/hard/per-symbol thresholds without standing up a bespoke
    /// schema migration.
    pub fn with_caps(
        db: Arc<Db>,
        soft_cap: u32,
        hard_cap: u32,
        per_symbol_cap: u32,
        date_source: Arc<dyn DateSource>,
    ) -> Self {
        Self {
            db,
            soft_cap,
            hard_cap,
            per_symbol_cap,
            daily_counts: Mutex::new(HashMap::new()),
            per_symbol_counts: Mutex::new(HashMap::new()),
            date_source,
        }
    }

    /// Check whether an AV call for `symbol` is permitted today without
    /// mutating the ledger. The composite provider's AV branch calls
    /// this BEFORE delegating to the AV provider; on a successful AV
    /// call it follows up with [`Self::commit`]. Splitting the check
    /// from the increment keeps a failed AV call from burning a ticket
    /// (so a transport error doesn't silently exhaust the daily quota).
    ///
    /// Counts are 1-per-`composite.fetch`, NOT 1-per-endpoint. The AV
    /// adapter under the hood fans out 3 endpoint calls (`OVERVIEW` /
    /// `INCOME_STATEMENT` / `EARNINGS`) — we count this as one
    /// operator-cost unit.
    pub async fn check(&self, symbol: &str) -> Result<ReserveOutcome, AvLedgerError> {
        let symbol = symbol.trim().to_uppercase();
        if symbol.is_empty() {
            return Err(AvLedgerError::Storage(StorageError::Migration(
                "AvCallLedger.check: symbol must be non-empty".to_string(),
            )));
        }
        let today = self.date_source.today();
        self.hydrate_if_empty(today).await?;

        let per_symbol_count = {
            let map = self
                .per_symbol_counts
                .lock()
                .expect("AvCallLedger per-symbol mutex poisoned");
            *map.get(&(today, symbol.clone())).unwrap_or(&0)
        };
        if per_symbol_count >= self.per_symbol_cap {
            return Err(AvLedgerError::PerSymbolCapReached {
                symbol,
                count: per_symbol_count,
            });
        }

        let daily_count = {
            let map = self
                .daily_counts
                .lock()
                .expect("AvCallLedger daily mutex poisoned");
            *map.get(&today).unwrap_or(&0)
        };
        if daily_count >= self.hard_cap {
            return Err(AvLedgerError::DailyCapReached {
                hit_count: daily_count,
            });
        }

        // The next increment will land at daily_count+1; soft-cap is a
        // strict-greater check (>20 emits, =20 doesn't) so the trip
        // wire fires precisely at the 21st call.
        if daily_count + 1 > self.soft_cap {
            Ok(ReserveOutcome::AboveSoftCap)
        } else {
            Ok(ReserveOutcome::BelowSoftCap)
        }
    }

    /// Commit one AV call for `symbol` to the ledger. Called by the
    /// composite provider's AV branch only after a successful upstream
    /// fetch. The increment is persisted to SQLite first; in-memory
    /// counters are bumped only after the row write succeeds so disk
    /// and memory cannot drift on a failed write.
    pub async fn commit(&self, symbol: &str) -> Result<(), AvLedgerError> {
        let symbol = symbol.trim().to_uppercase();
        if symbol.is_empty() {
            return Err(AvLedgerError::Storage(StorageError::Migration(
                "AvCallLedger.commit: symbol must be non-empty".to_string(),
            )));
        }
        let today = self.date_source.today();
        self.hydrate_if_empty(today).await?;

        let now_ts = chrono::Utc::now().timestamp();
        let date_str = today.format("%Y-%m-%d").to_string();
        let symbol_for_db = symbol.clone();
        self.db
            .with_conn(move |conn| {
                let tx = conn.transaction()?;
                tx.execute(
                    "INSERT INTO av_call_ledger (date, count, updated_at) \
                     VALUES (?1, 1, ?2) \
                     ON CONFLICT(date) DO UPDATE SET \
                       count = count + 1, \
                       updated_at = excluded.updated_at",
                    rusqlite::params![date_str, now_ts],
                )?;
                tx.execute(
                    "INSERT INTO av_per_symbol_ledger (date, symbol, count, updated_at) \
                     VALUES (?1, ?2, 1, ?3) \
                     ON CONFLICT(date, symbol) DO UPDATE SET \
                       count = count + 1, \
                       updated_at = excluded.updated_at",
                    rusqlite::params![date_str, symbol_for_db, now_ts],
                )?;
                tx.commit()?;
                Ok(())
            })
            .await?;

        let new_daily = {
            let mut map = self
                .daily_counts
                .lock()
                .expect("AvCallLedger daily mutex poisoned");
            let entry = map.entry(today).or_insert(0);
            *entry += 1;
            *entry
        };
        {
            let mut map = self
                .per_symbol_counts
                .lock()
                .expect("AvCallLedger per-symbol mutex poisoned");
            let entry = map.entry((today, symbol.clone())).or_insert(0);
            *entry += 1;
        }

        if new_daily > self.soft_cap {
            warn!(
                soft_cap = self.soft_cap,
                hard_cap = self.hard_cap,
                hit_count = new_daily,
                symbol = %symbol,
                "AV call ledger past daily soft cap (trip-wire) — investigate why background work is reaching AV"
            );
        }
        Ok(())
    }

    /// Read the current daily count for inspection (UI banner / tests).
    /// Hydrates from SQLite if the in-memory map is empty for `today`.
    #[allow(dead_code)] // exposed for the planned UI banner; only tests consume it today
    pub async fn daily_count_today(&self) -> Result<u32, AvLedgerError> {
        let today = self.date_source.today();
        self.hydrate_if_empty(today).await?;
        let count = self
            .daily_counts
            .lock()
            .expect("AvCallLedger daily mutex poisoned")
            .get(&today)
            .copied()
            .unwrap_or(0);
        Ok(count)
    }

    /// Read the current per-symbol count for inspection / tests.
    #[allow(dead_code)] // exposed for the planned UI banner; only tests consume it today
    pub async fn per_symbol_count_today(&self, symbol: &str) -> Result<u32, AvLedgerError> {
        let symbol = symbol.trim().to_uppercase();
        let today = self.date_source.today();
        self.hydrate_if_empty(today).await?;
        let count = self
            .per_symbol_counts
            .lock()
            .expect("AvCallLedger per-symbol mutex poisoned")
            .get(&(today, symbol))
            .copied()
            .unwrap_or(0);
        Ok(count)
    }

    pub fn soft_cap(&self) -> u32 {
        self.soft_cap
    }

    pub fn hard_cap(&self) -> u32 {
        self.hard_cap
    }

    #[allow(dead_code)] // exposed for symmetry with soft/hard cap accessors
    pub fn per_symbol_cap(&self) -> u32 {
        self.per_symbol_cap
    }

    /// On the first reserve / read for a given calendar date, pull any
    /// persisted count from SQLite into the in-memory map. Subsequent
    /// calls during the same day skip this (the in-memory map is the
    /// source of truth between writes). The hydrate path is the only
    /// reason a process restart preserves the ledger across crashes.
    async fn hydrate_if_empty(&self, today: NaiveDate) -> Result<(), AvLedgerError> {
        let needs_hydrate = {
            let daily_map = self
                .daily_counts
                .lock()
                .expect("AvCallLedger daily mutex poisoned");
            !daily_map.contains_key(&today)
        };
        if !needs_hydrate {
            return Ok(());
        }
        let date_str = today.format("%Y-%m-%d").to_string();
        let date_for_query = date_str.clone();
        let (daily, per_symbol) = self
            .db
            .with_conn(move |conn| {
                let daily: u32 = conn
                    .query_row(
                        "SELECT count FROM av_call_ledger WHERE date = ?1",
                        rusqlite::params![date_for_query],
                        |row| row.get::<_, i64>(0),
                    )
                    .map(|v| v.max(0) as u32)
                    .or_else(|e| match e {
                        rusqlite::Error::QueryReturnedNoRows => Ok(0u32),
                        other => Err(other),
                    })?;
                let mut stmt =
                    conn.prepare("SELECT symbol, count FROM av_per_symbol_ledger WHERE date = ?1")?;
                let rows = stmt
                    .query_map(rusqlite::params![date_for_query], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, i64>(1)?.max(0) as u32,
                        ))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok((daily, rows))
            })
            .await?;
        {
            let mut daily_map = self
                .daily_counts
                .lock()
                .expect("AvCallLedger daily mutex poisoned");
            daily_map.insert(today, daily);
        }
        {
            let mut per_symbol_map = self
                .per_symbol_counts
                .lock()
                .expect("AvCallLedger per-symbol mutex poisoned");
            for (symbol, count) in per_symbol {
                per_symbol_map.insert((today, symbol), count);
            }
        }
        Ok(())
    }
}
