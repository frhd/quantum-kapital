//! Phase 7 — outcome_extractor.
//!
//! Given an agent-authored morning-pack `RankedIdea` (entry_zone +
//! invalidation as free-form strings) and a list of bars covering the
//! evaluation window, classifies the prediction into one of the closed
//! [`OutcomeClass`] variants and persists a row to `outcomes` keyed by
//! `(pack_date, symbol)`.
//!
//! Two orthogonal pieces:
//!
//! 1. **Parsing.** [`parse_entry_zone`] / [`parse_invalidation`] pull
//!    numbers out of the loose strings the agent emits ("100-105",
//!    "close < 95"). Both return `None` if nothing parseable is found —
//!    callers downgrade to [`OutcomeClass::Unparseable`].
//! 2. **Classification.** [`classify`] takes the parsed levels + a bars
//!    summary (high/low/close + windowed daily-bar slice) and returns
//!    the outcome class. Pure function; the persistence layer
//!    ([`record_outcome`]) lives separately so tests can assert
//!    classification logic without touching SQLite.
//!
//! `hit_target` is intentionally conservative: ideas don't carry an
//! explicit target level, so the heuristic credits "hit_target" only
//! when price moved past `entry_high + target_multiplier * range`
//! after entering the zone — this falls back to `hit_entry` when no
//! sensible target can be derived (e.g. the entry zone is a single
//! point).
//!
//! Re-running the EOD review for the same pack date overwrites the
//! existing outcomes row — `record_outcome` upserts on
//! `(pack_date, symbol)`.

use std::sync::Arc;

use chrono::{DateTime, NaiveDate, Utc};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::ibkr::types::historical::{parse_ibkr_time, HistoricalBar};
use crate::services::agent_morning_packs::RankedIdea;
use crate::services::research_notes::Conviction;
use crate::storage::error::StorageError;
use crate::storage::Db;
use crate::utils::helpers::unix_to_utc;

#[cfg(test)]
mod tests;

/// Closed-by-design outcome taxonomy — see Phase 7 plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeClass {
    HitEntry,
    HitTarget,
    HitInvalidation,
    Drifted,
    NoMovement,
    /// The pack noted "no high-conviction picks today" — there's no
    /// price action to score. Recorded so survivorship doesn't bias
    /// the eval harness.
    Skipped,
    /// Entry / invalidation strings could not be parsed. Tracked
    /// rather than dropped so prompt iteration sees the failure.
    Unparseable,
}

impl OutcomeClass {
    pub fn as_str(&self) -> &'static str {
        match self {
            OutcomeClass::HitEntry => "hit_entry",
            OutcomeClass::HitTarget => "hit_target",
            OutcomeClass::HitInvalidation => "hit_invalidation",
            OutcomeClass::Drifted => "drifted",
            OutcomeClass::NoMovement => "no_movement",
            OutcomeClass::Skipped => "skipped",
            OutcomeClass::Unparseable => "unparseable",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "hit_entry" => Some(OutcomeClass::HitEntry),
            "hit_target" => Some(OutcomeClass::HitTarget),
            "hit_invalidation" => Some(OutcomeClass::HitInvalidation),
            "drifted" => Some(OutcomeClass::Drifted),
            "no_movement" => Some(OutcomeClass::NoMovement),
            "skipped" => Some(OutcomeClass::Skipped),
            "unparseable" => Some(OutcomeClass::Unparseable),
            _ => None,
        }
    }
}

/// Numeric levels parsed from a [`RankedIdea`]. Either bound on the
/// entry zone may be `None` if only one point was parseable; in that
/// case [`classify`] treats the zone as a single point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParsedLevels {
    pub entry_zone_low: Option<f64>,
    pub entry_zone_high: Option<f64>,
    pub invalidation: Option<f64>,
}

impl ParsedLevels {
    fn entry_pair(&self) -> Option<(f64, f64)> {
        match (self.entry_zone_low, self.entry_zone_high) {
            (Some(lo), Some(hi)) if lo <= hi => Some((lo, hi)),
            (Some(lo), Some(hi)) => Some((hi, lo)),
            (Some(p), None) | (None, Some(p)) => Some((p, p)),
            (None, None) => None,
        }
    }
}

/// Tunable heuristics for [`classify`]. Captured in one place so the
/// agent loop can override per-symbol if ever needed (currently
/// fixed at construction time).
#[derive(Debug, Clone, Copy)]
pub struct OutcomeExtractorConfig {
    /// Half-width (as a fraction of the zone midpoint) within which
    /// a `realized_high` / `realized_low` counts as "no movement".
    /// Default: 0.005 (±0.5%) per Phase 7 plan.
    pub no_movement_pct: f64,
    /// `hit_target` requires price to exceed
    /// `entry_high + target_multiplier * (entry_high - entry_low)`
    /// (long ideas) — i.e. the same width as the entry zone, scaled by
    /// the multiplier. Defaults to 2.0 — a 2× zone-width move beyond
    /// entry is a meaningful follow-through.
    pub target_multiplier: f64,
}

impl Default for OutcomeExtractorConfig {
    fn default() -> Self {
        Self {
            no_movement_pct: 0.005,
            target_multiplier: 2.0,
        }
    }
}

/// Realized price action for a window around the pack date.
///
/// Built once from the bars returned by `HistoricalDataService` so the
/// classifier doesn't re-walk the full `Vec<HistoricalBar>` for each
/// strand of logic.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RealizedAction {
    pub high: f64,
    pub low: f64,
    pub close: f64,
}

impl RealizedAction {
    /// Aggregate `bars` into a single `(high, low, close)` triple.
    /// `None` if the slice is empty — the caller decides what to do
    /// (typically: mark `outcome=skipped` because no data was
    /// available to score).
    pub fn from_bars(bars: &[HistoricalBar]) -> Option<Self> {
        if bars.is_empty() {
            return None;
        }
        let mut hi = f64::MIN;
        let mut lo = f64::MAX;
        let mut last_close: Option<f64> = None;
        let mut last_ts: i64 = i64::MIN;
        for b in bars {
            if b.high > hi {
                hi = b.high;
            }
            if b.low < lo {
                lo = b.low;
            }
            let ts = parse_ibkr_time(&b.time).unwrap_or(i64::MIN);
            if ts >= last_ts {
                last_ts = ts;
                last_close = Some(b.close);
            }
        }
        Some(Self {
            high: hi,
            low: lo,
            close: last_close.unwrap_or(0.0),
        })
    }
}

/// Pure classifier — no I/O. Phase-7 logic lives here so the EOD
/// agent loop can be tested entirely against a [`HistoricalBar`]
/// fixture.
///
/// `idea_skipped` is `true` when the morning pack noted "no
/// high-conviction picks today" — currently the agent surfaces that
/// by writing a [`RankedIdea`] whose `thesis_md` starts with `"SKIP:"`,
/// but the classifier just trusts the caller. Keeps the rule out of
/// the heuristics module.
pub fn classify(
    levels: &ParsedLevels,
    realized: &RealizedAction,
    cfg: &OutcomeExtractorConfig,
    idea_skipped: bool,
) -> OutcomeClass {
    if idea_skipped {
        return OutcomeClass::Skipped;
    }
    let (entry_low, entry_high) = match levels.entry_pair() {
        Some(pair) => pair,
        None => return OutcomeClass::Unparseable,
    };

    // Invalidation hit when realized_low (long) crosses the level on
    // the way down. Pure-direction-agnostic: we treat any cross of
    // `invalidation_lvl` by the day's range as a hit. The agent
    // typically expresses invalidation as "close < X" — we use the
    // tighter `realized_low <= X` (i.e. the tape touched X) to be
    // conservative.
    if let Some(inv) = levels.invalidation {
        if realized.low <= inv {
            return OutcomeClass::HitInvalidation;
        }
    }

    // No-movement check sits *above* the entry-overlap branch so a
    // tight ±band hugging the zone counts as "the day did nothing"
    // rather than a meaningful entry — the day's range never
    // breached the zone in either direction beyond the noise band.
    // Boundaries are scaled off the zone bounds (not the midpoint)
    // so wider zones don't get an artificially narrow band.
    let pct = cfg.no_movement_pct;
    if entry_low > 0.0 && entry_high > 0.0 {
        let lo_floor = entry_low * (1.0 - pct);
        let hi_ceil = entry_high * (1.0 + pct);
        if realized.low >= lo_floor && realized.high <= hi_ceil {
            return OutcomeClass::NoMovement;
        }
    }

    // Entry hit when the bar's [low, high] overlaps the zone.
    let entered = realized.high >= entry_low && realized.low <= entry_high;

    if entered {
        // Try to credit hit_target on top of hit_entry: price needed
        // to push past `entry_high + target_multiplier * range`.
        // Single-point zones (low == high) have zero range, so
        // target classification falls back to hit_entry.
        let range = entry_high - entry_low;
        if range > 0.0 {
            let target = entry_high + cfg.target_multiplier * range;
            if realized.high >= target {
                return OutcomeClass::HitTarget;
            }
        }
        return OutcomeClass::HitEntry;
    }

    OutcomeClass::Drifted
}

/// Pull a `(low, high)` pair (or single point) out of the agent's
/// `entry_zone` string. Returns `None` if no number could be
/// extracted.
///
/// Recognises the common shapes: `"100-105"`, `"100 to 105"`,
/// `"~100"`, `"$100.50"`, `"100"` (single point), and tolerates
/// surrounding prose. Numeric extraction picks up the first two
/// finite floats it finds.
pub fn parse_entry_zone(s: &str) -> ParsedLevels {
    let nums = extract_numbers(s);
    let (low, high) = match nums.as_slice() {
        [] => (None, None),
        [p] => (Some(*p), Some(*p)),
        [a, b, ..] => {
            let (lo, hi) = if a <= b { (*a, *b) } else { (*b, *a) };
            (Some(lo), Some(hi))
        }
    };
    ParsedLevels {
        entry_zone_low: low,
        entry_zone_high: high,
        invalidation: None,
    }
}

/// Pull the invalidation level out of the agent's free-form string.
/// First number wins — agents typically write "close < 95" or
/// "below 95" or just "95".
pub fn parse_invalidation(s: &str) -> Option<f64> {
    extract_numbers(s).into_iter().next()
}

/// Combine an idea's `entry_zone` + `invalidation` strings into a
/// single [`ParsedLevels`] handle.
pub fn parse_idea(idea: &RankedIdea) -> ParsedLevels {
    let mut levels = match idea.entry_zone.as_deref() {
        Some(s) => parse_entry_zone(s),
        None => ParsedLevels {
            entry_zone_low: None,
            entry_zone_high: None,
            invalidation: None,
        },
    };
    levels.invalidation = idea.invalidation.as_deref().and_then(parse_invalidation);
    levels
}

fn extract_numbers(s: &str) -> Vec<f64> {
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        // A `-` is a sign only when it is not preceded by a digit or
        // dot — otherwise it's a separator (e.g. "100-105").
        let minus_is_sign =
            c == b'-' && next_is_digit(bytes, i + 1) && (i == 0 || !is_numeric_byte(bytes[i - 1]));
        if c.is_ascii_digit() || c == b'.' || minus_is_sign {
            let start = i;
            if c == b'-' {
                i += 1;
            }
            let mut saw_dot = false;
            while i < bytes.len() {
                let d = bytes[i];
                if d.is_ascii_digit() {
                    i += 1;
                } else if d == b'.' && !saw_dot {
                    saw_dot = true;
                    i += 1;
                } else {
                    break;
                }
            }
            let token = &s[start..i];
            if let Ok(v) = token.parse::<f64>() {
                if v.is_finite() {
                    out.push(v);
                }
            }
        } else {
            i += 1;
        }
    }
    out
}

fn next_is_digit(bytes: &[u8], i: usize) -> bool {
    i < bytes.len() && bytes[i].is_ascii_digit()
}

fn is_numeric_byte(b: u8) -> bool {
    b.is_ascii_digit() || b == b'.'
}

// ---------- persistence ----------

/// Persisted form of a scored prediction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutcomeRow {
    pub id: i64,
    pub pack_date: NaiveDate,
    pub symbol: String,
    pub outcome_class: OutcomeClass,
    pub conviction: Option<Conviction>,
    pub entry_zone_low: Option<f64>,
    pub entry_zone_high: Option<f64>,
    pub invalidation_lvl: Option<f64>,
    pub realized_high: f64,
    pub realized_low: f64,
    pub realized_close: f64,
    pub eval_window_days: i64,
    pub evaluated_at: DateTime<Utc>,
}

/// Inputs for [`record_outcome`]. Built once per (pack, symbol) by
/// the EOD agent loop.
#[derive(Debug, Clone)]
pub struct NewOutcome {
    pub pack_date: NaiveDate,
    pub symbol: String,
    pub outcome_class: OutcomeClass,
    pub conviction: Option<Conviction>,
    pub entry_zone_low: Option<f64>,
    pub entry_zone_high: Option<f64>,
    pub invalidation_lvl: Option<f64>,
    pub realized_high: f64,
    pub realized_low: f64,
    pub realized_close: f64,
    pub eval_window_days: i64,
}

#[derive(Debug, Error)]
pub enum OutcomeError {
    #[error("storage: {0}")]
    Storage(#[from] StorageError),
}

/// Upsert one outcome row keyed by `(pack_date, symbol)`.
pub async fn record_outcome(db: &Arc<Db>, new: NewOutcome) -> Result<OutcomeRow, OutcomeError> {
    let now_unix = Utc::now().timestamp();
    let pack_date_str = new.pack_date.to_string();
    let symbol_norm = new.symbol.to_uppercase();
    let outcome_str = new.outcome_class.as_str().to_string();
    let conviction_str = new.conviction.map(|c| c.as_str().to_string());

    let symbol_for_db = symbol_norm.clone();
    let pack_date_for_db = pack_date_str.clone();
    let outcome_for_db = outcome_str.clone();
    let conviction_for_db = conviction_str.clone();
    let entry_lo = new.entry_zone_low;
    let entry_hi = new.entry_zone_high;
    let inv = new.invalidation_lvl;
    let high = new.realized_high;
    let low = new.realized_low;
    let close = new.realized_close;
    let window = new.eval_window_days;

    let id = db
        .with_conn(move |conn| {
            conn.execute(
                "INSERT INTO outcomes \
                 (pack_date, symbol, outcome_class, conviction, \
                  entry_zone_low, entry_zone_high, invalidation_lvl, \
                  realized_high, realized_low, realized_close, \
                  eval_window_days, evaluated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12) \
                 ON CONFLICT(pack_date, symbol) DO UPDATE SET \
                     outcome_class    = excluded.outcome_class, \
                     conviction       = excluded.conviction, \
                     entry_zone_low   = excluded.entry_zone_low, \
                     entry_zone_high  = excluded.entry_zone_high, \
                     invalidation_lvl = excluded.invalidation_lvl, \
                     realized_high    = excluded.realized_high, \
                     realized_low     = excluded.realized_low, \
                     realized_close   = excluded.realized_close, \
                     eval_window_days = excluded.eval_window_days, \
                     evaluated_at     = excluded.evaluated_at",
                rusqlite::params![
                    pack_date_for_db,
                    symbol_for_db,
                    outcome_for_db,
                    conviction_for_db,
                    entry_lo,
                    entry_hi,
                    inv,
                    high,
                    low,
                    close,
                    window,
                    now_unix,
                ],
            )?;
            let id: i64 = conn.query_row(
                "SELECT id FROM outcomes WHERE pack_date = ?1 AND symbol = ?2",
                rusqlite::params![pack_date_str, symbol_norm],
                |row| row.get(0),
            )?;
            Ok(id)
        })
        .await?;

    Ok(OutcomeRow {
        id,
        pack_date: new.pack_date,
        symbol: new.symbol.to_uppercase(),
        outcome_class: new.outcome_class,
        conviction: new.conviction,
        entry_zone_low: new.entry_zone_low,
        entry_zone_high: new.entry_zone_high,
        invalidation_lvl: new.invalidation_lvl,
        realized_high: new.realized_high,
        realized_low: new.realized_low,
        realized_close: new.realized_close,
        eval_window_days: new.eval_window_days,
        evaluated_at: unix_to_utc(now_unix),
    })
}

/// Read outcome rows since (and including) `since`. Newest pack-date
/// first. Used by the `get_outcomes` MCP read tool.
pub async fn list_outcomes_since(
    db: &Arc<Db>,
    since: NaiveDate,
) -> Result<Vec<OutcomeRow>, OutcomeError> {
    let since_str = since.to_string();
    let raws = db
        .with_conn(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id, pack_date, symbol, outcome_class, conviction, \
                        entry_zone_low, entry_zone_high, invalidation_lvl, \
                        realized_high, realized_low, realized_close, \
                        eval_window_days, evaluated_at \
                 FROM outcomes WHERE pack_date >= ?1 \
                 ORDER BY pack_date DESC, id ASC",
            )?;
            let rows = stmt
                .query_map(rusqlite::params![since_str], row_to_raw)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .await?;
    raws.into_iter().map(decode_raw).collect()
}

/// Read outcome row by `(pack_date, symbol)`. Returns `Ok(None)` when
/// the row is absent.
#[allow(dead_code)] // covered by record_outcome upsert tests; reserved
                    // for the eval harness in Phase 8.
pub async fn get_outcome(
    db: &Arc<Db>,
    pack_date: NaiveDate,
    symbol: &str,
) -> Result<Option<OutcomeRow>, OutcomeError> {
    let pack_date_str = pack_date.to_string();
    let symbol_norm = symbol.to_uppercase();
    let raw = db
        .with_conn(move |conn| {
            conn.query_row(
                "SELECT id, pack_date, symbol, outcome_class, conviction, \
                        entry_zone_low, entry_zone_high, invalidation_lvl, \
                        realized_high, realized_low, realized_close, \
                        eval_window_days, evaluated_at \
                 FROM outcomes WHERE pack_date = ?1 AND symbol = ?2",
                rusqlite::params![pack_date_str, symbol_norm],
                row_to_raw,
            )
            .optional()
            .map_err(StorageError::from)
        })
        .await?;
    raw.map(decode_raw).transpose()
}

type RawRow = (
    i64,
    String,
    String,
    String,
    Option<String>,
    Option<f64>,
    Option<f64>,
    Option<f64>,
    f64,
    f64,
    f64,
    i64,
    i64,
);

fn row_to_raw(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawRow> {
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
    ))
}

fn decode_raw(r: RawRow) -> Result<OutcomeRow, OutcomeError> {
    let (
        id,
        pack_date_s,
        symbol,
        outcome_s,
        conviction_s,
        entry_lo,
        entry_hi,
        inv,
        hi,
        lo,
        close,
        window,
        evaluated_at,
    ) = r;
    let pack_date = NaiveDate::parse_from_str(&pack_date_s, "%Y-%m-%d").map_err(|e| {
        OutcomeError::Storage(StorageError::Migration(format!(
            "invalid pack_date '{pack_date_s}': {e}"
        )))
    })?;
    let outcome_class = OutcomeClass::parse(&outcome_s).ok_or_else(|| {
        OutcomeError::Storage(StorageError::Migration(format!(
            "invalid outcome_class '{outcome_s}'"
        )))
    })?;
    let conviction = conviction_s.as_deref().and_then(Conviction::parse);
    Ok(OutcomeRow {
        id,
        pack_date,
        symbol,
        outcome_class,
        conviction,
        entry_zone_low: entry_lo,
        entry_zone_high: entry_hi,
        invalidation_lvl: inv,
        realized_high: hi,
        realized_low: lo,
        realized_close: close,
        eval_window_days: window,
        evaluated_at: unix_to_utc(evaluated_at),
    })
}
