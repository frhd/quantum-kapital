//! `TradeReviewStore` — writer + reader for the `day_reviews` table.
//!
//! Idempotent UPSERT keyed on `(date, account, prompt_version)`. Phase
//! 4 split: writes are now v2 by default — `score_v2` /
//! `discipline_v2` / `risk_metrics_json` / `equity_curve_json` /
//! `formula_version='v2'`. The legacy `(grade, grade_score)` columns
//! relax to NULLABLE in V20 and stay NULL on v2 rows. Pre-P4 rows
//! continue to read back with their stored v1 values and
//! `formula_version='v1'`. Caller passes a [`ReviewV2Fields`] alongside
//! the existing summary; pre-P4 callers can pass
//! [`ReviewV2Fields::v1_only`] for a passthrough write that keeps the
//! row on the legacy schema.

use std::sync::Arc;

use chrono::{DateTime, NaiveDate, Utc};
use rusqlite::{params, OptionalExtension};
use thiserror::Error;

use crate::storage::error::StorageError;
use crate::storage::Db;

use super::equity_curve::EquityPoint;
use super::grade::GradeLetter;
use super::risk_metrics::RiskMetrics;
use super::tags::BehavioralTag;
use super::types::{
    LegObservation, LegSummary, ReviewV2Fields, TradeReview, WriteTradeReviewRequest,
};

#[derive(Error, Debug)]
pub enum TradeReviewError {
    #[error("storage: {0}")]
    Storage(#[from] StorageError),
    #[error("serde: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("invalid persisted row: {0}")]
    InvalidRow(String),
    #[error("account must be non-empty")]
    EmptyAccount,
    #[error("narrative_md must be non-empty")]
    EmptyNarrative,
}

/// Outcome of a write.
///
/// `score_v2` / `discipline_v2` are surfaced separately — callers MUST
/// NOT sum them for ranking (master commitment).
#[derive(Debug, Clone)]
pub struct WriteOutcome {
    pub review: TradeReview,
    pub score_v2: Option<f64>,
    pub discipline_v2: Option<f64>,
    pub formula_version: String,
}

#[derive(Clone)]
pub struct TradeReviewStore {
    db: Arc<Db>,
}

impl TradeReviewStore {
    pub fn new(db: Arc<Db>) -> Self {
        Self { db }
    }

    /// UPSERT a trade review for `(date, account, prompt_version)`
    /// with explicit v2 fields. Pre-P4 callers can pass
    /// `ReviewV2Fields::v1_only()` to write a v1-tagged row with
    /// NULL v2 numerics — pre-existing rows are not retroactively
    /// upgraded.
    pub async fn write(
        &self,
        req: WriteTradeReviewRequest,
        v2: ReviewV2Fields,
    ) -> Result<WriteOutcome, TradeReviewError> {
        if req.account.trim().is_empty() {
            return Err(TradeReviewError::EmptyAccount);
        }
        if req.narrative_md.trim().is_empty() {
            return Err(TradeReviewError::EmptyNarrative);
        }

        let now = Utc::now();
        let date_str = req.date.to_string();
        let account = req.account.clone();
        let prompt_version = req.prompt_version;
        let generated_at_str = now.to_rfc3339();
        let summary_json = serde_json::to_string(&req.summary)?;
        let tags_json = serde_json::to_string(&req.behavioral_tags)?;
        let observations_json = serde_json::to_string(&req.leg_observations)?;
        let narrative = req.narrative_md.clone();
        let llm_call_id = req.llm_call_id.clone();
        let summary_for_row = req.summary.clone();
        let tags_for_row = req.behavioral_tags.clone();
        let observations_for_row = req.leg_observations.clone();

        let risk_metrics_json = match &v2.risk_metrics {
            Some(m) => Some(serde_json::to_string(m)?),
            None => None,
        };
        let equity_curve_json = match &v2.equity_curve {
            Some(c) => Some(serde_json::to_string(c)?),
            None => None,
        };
        let formula_version = v2.formula_version.clone();
        let score_v2_value = v2.score_v2;
        let discipline_v2_value = v2.discipline_v2;
        let risk_metrics_clone = v2.risk_metrics.clone();
        let equity_curve_clone = v2.equity_curve.clone();

        self.db
            .with_conn(move |conn| {
                conn.execute(
                    "INSERT INTO day_reviews (
                        date, account, prompt_version, generated_at, grade, grade_score,
                        gross_pnl, net_pnl, commissions_total, n_round_trips, n_carryover,
                        win_rate, behavioral_tags, leg_observations, summary_json,
                        narrative_md, llm_call_id,
                        score_v2, discipline_v2, risk_metrics_json, equity_curve_json,
                        formula_version
                     ) VALUES (
                        ?1, ?2, ?3, ?4, NULL, NULL, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                        ?14, ?15, ?16, ?17, ?18, ?19, ?20
                     )
                     ON CONFLICT(date, account, prompt_version) DO UPDATE SET
                        generated_at      = excluded.generated_at,
                        grade             = excluded.grade,
                        grade_score       = excluded.grade_score,
                        gross_pnl         = excluded.gross_pnl,
                        net_pnl           = excluded.net_pnl,
                        commissions_total = excluded.commissions_total,
                        n_round_trips     = excluded.n_round_trips,
                        n_carryover       = excluded.n_carryover,
                        win_rate          = excluded.win_rate,
                        behavioral_tags   = excluded.behavioral_tags,
                        leg_observations  = excluded.leg_observations,
                        summary_json      = excluded.summary_json,
                        narrative_md      = excluded.narrative_md,
                        llm_call_id       = excluded.llm_call_id,
                        score_v2          = excluded.score_v2,
                        discipline_v2     = excluded.discipline_v2,
                        risk_metrics_json = excluded.risk_metrics_json,
                        equity_curve_json = excluded.equity_curve_json,
                        formula_version   = excluded.formula_version",
                    params![
                        date_str,
                        account,
                        prompt_version,
                        generated_at_str,
                        summary_for_row.gross_pnl,
                        summary_for_row.net_pnl,
                        summary_for_row.commissions_total,
                        summary_for_row.n_round_trips as i64,
                        summary_for_row.n_carryover as i64,
                        summary_for_row.win_rate,
                        tags_json,
                        observations_json,
                        summary_json,
                        narrative,
                        llm_call_id,
                        score_v2_value,
                        discipline_v2_value,
                        risk_metrics_json,
                        equity_curve_json,
                        formula_version,
                    ],
                )?;
                Ok(())
            })
            .await?;

        Ok(WriteOutcome {
            review: TradeReview {
                date: req.date,
                account: req.account,
                prompt_version: req.prompt_version,
                generated_at: now,
                formula_version: v2.formula_version.clone(),
                grade: None,
                grade_score: None,
                score_v2: score_v2_value,
                discipline_v2: discipline_v2_value,
                risk_metrics: risk_metrics_clone,
                equity_curve: equity_curve_clone,
                summary: summary_for_row,
                behavioral_tags: tags_for_row,
                leg_observations: observations_for_row,
                narrative_md: req.narrative_md,
                llm_call_id: req.llm_call_id,
            },
            score_v2: score_v2_value,
            discipline_v2: discipline_v2_value,
            formula_version: v2.formula_version,
        })
    }

    /// Read a review for `(date, account, prompt_version)`. Returns
    /// `Ok(None)` when no row exists.
    pub async fn read(
        &self,
        date: NaiveDate,
        account: &str,
        prompt_version: i32,
    ) -> Result<Option<TradeReview>, TradeReviewError> {
        let date_str = date.to_string();
        let account = account.to_string();
        let row: Option<RawRow> = self
            .db
            .with_conn(move |conn| {
                conn.query_row(
                    SELECT_COLUMNS,
                    params![date_str, account, prompt_version],
                    map_raw,
                )
                .optional()
                .map_err(StorageError::from)
            })
            .await?;
        row.map(parse_row).transpose()
    }

    /// Read the latest `prompt_version` row for `(date, account)`.
    /// Returns `Ok(None)` when no row exists.
    pub async fn read_latest(
        &self,
        date: NaiveDate,
        account: &str,
    ) -> Result<Option<TradeReview>, TradeReviewError> {
        let date_str = date.to_string();
        let account = account.to_string();
        let row: Option<RawRow> = self
            .db
            .with_conn(move |conn| {
                conn.query_row(
                    SELECT_COLUMNS_LATEST,
                    params![date_str, account],
                    map_raw,
                )
                .optional()
                .map_err(StorageError::from)
            })
            .await?;
        row.map(parse_row).transpose()
    }

    /// Count rows in the table — observability hook used by tests.
    #[cfg(test)]
    pub async fn count(&self) -> Result<i64, TradeReviewError> {
        let n: i64 = self
            .db
            .with_conn(|conn| {
                let n: i64 =
                    conn.query_row("SELECT COUNT(*) FROM day_reviews", [], |r| r.get(0))?;
                Ok(n)
            })
            .await?;
        Ok(n)
    }
}

const SELECT_COLUMNS: &str = "SELECT date, account, prompt_version, generated_at, grade, grade_score,
        gross_pnl, net_pnl, commissions_total, n_round_trips, n_carryover,
        win_rate, behavioral_tags, leg_observations, summary_json,
        narrative_md, llm_call_id,
        score_v2, discipline_v2, risk_metrics_json, equity_curve_json,
        formula_version
        FROM day_reviews
        WHERE date = ?1 AND account = ?2 AND prompt_version = ?3";

const SELECT_COLUMNS_LATEST: &str = "SELECT date, account, prompt_version, generated_at, grade, grade_score,
        gross_pnl, net_pnl, commissions_total, n_round_trips, n_carryover,
        win_rate, behavioral_tags, leg_observations, summary_json,
        narrative_md, llm_call_id,
        score_v2, discipline_v2, risk_metrics_json, equity_curve_json,
        formula_version
        FROM day_reviews
        WHERE date = ?1 AND account = ?2
        ORDER BY prompt_version DESC
        LIMIT 1";

struct RawRow {
    date: String,
    account: String,
    prompt_version: i32,
    generated_at: String,
    grade: Option<String>,
    grade_score: Option<f64>,
    win_rate: Option<f64>,
    behavioral_tags: String,
    leg_observations: String,
    summary_json: String,
    narrative_md: String,
    llm_call_id: Option<String>,
    score_v2: Option<f64>,
    discipline_v2: Option<f64>,
    risk_metrics_json: Option<String>,
    equity_curve_json: Option<String>,
    formula_version: String,
}

fn map_raw(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawRow> {
    Ok(RawRow {
        date: row.get(0)?,
        account: row.get(1)?,
        prompt_version: row.get(2)?,
        generated_at: row.get(3)?,
        grade: row.get(4)?,
        grade_score: row.get(5)?,
        // skip 6..=10 (denormalised numerics — read back via summary_json)
        win_rate: row.get(11)?,
        behavioral_tags: row.get(12)?,
        leg_observations: row.get(13)?,
        summary_json: row.get(14)?,
        narrative_md: row.get(15)?,
        llm_call_id: row.get(16)?,
        score_v2: row.get(17)?,
        discipline_v2: row.get(18)?,
        risk_metrics_json: row.get(19)?,
        equity_curve_json: row.get(20)?,
        formula_version: row.get(21)?,
    })
}

fn parse_row(raw: RawRow) -> Result<TradeReview, TradeReviewError> {
    let date = NaiveDate::parse_from_str(&raw.date, "%Y-%m-%d")
        .map_err(|e| TradeReviewError::InvalidRow(format!("date `{}`: {e}", raw.date)))?;
    let generated_at = DateTime::parse_from_rfc3339(&raw.generated_at)
        .map_err(|e| {
            TradeReviewError::InvalidRow(format!("generated_at `{}`: {e}", raw.generated_at))
        })?
        .with_timezone(&Utc);
    let grade = match raw.grade {
        Some(g) => Some(GradeLetter::parse(&g).ok_or_else(|| {
            TradeReviewError::InvalidRow(format!("grade `{}`", g))
        })?),
        None => None,
    };
    let summary: LegSummary = serde_json::from_str(&raw.summary_json)?;
    let mut summary = summary;
    if summary.win_rate.is_none() {
        summary.win_rate = raw.win_rate;
    }
    let behavioral_tags: Vec<BehavioralTag> = serde_json::from_str(&raw.behavioral_tags)?;
    let leg_observations: Vec<LegObservation> = serde_json::from_str(&raw.leg_observations)?;
    let risk_metrics: Option<RiskMetrics> = match raw.risk_metrics_json {
        Some(s) => Some(serde_json::from_str(&s)?),
        None => None,
    };
    let equity_curve: Option<Vec<EquityPoint>> = match raw.equity_curve_json {
        Some(s) => Some(serde_json::from_str(&s)?),
        None => None,
    };
    Ok(TradeReview {
        date,
        account: raw.account,
        prompt_version: raw.prompt_version,
        generated_at,
        formula_version: raw.formula_version,
        grade,
        grade_score: raw.grade_score,
        score_v2: raw.score_v2,
        discipline_v2: raw.discipline_v2,
        risk_metrics,
        equity_curve,
        summary,
        behavioral_tags,
        leg_observations,
        narrative_md: raw.narrative_md,
        llm_call_id: raw.llm_call_id,
    })
}
