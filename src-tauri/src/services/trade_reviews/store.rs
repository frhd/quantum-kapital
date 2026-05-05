//! `TradeReviewStore` — writer + reader for the `day_reviews` table.
//!
//! Idempotent UPSERT keyed on `(date, account, prompt_version)`. The
//! grade is computed deterministically server-side at write time —
//! callers pass tags + summary, never a precomputed grade.

use std::sync::Arc;

use chrono::{DateTime, NaiveDate, Utc};
use rusqlite::{params, OptionalExtension};
use thiserror::Error;

use crate::storage::error::StorageError;
use crate::storage::Db;

use super::grade::{compute_grade, Grade, GradeLetter};
use super::tags::BehavioralTag;
use super::types::{LegObservation, LegSummary, TradeReview, WriteTradeReviewRequest};

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
#[derive(Debug, Clone)]
pub struct WriteOutcome {
    pub review: TradeReview,
    pub grade: Grade,
}

#[derive(Clone)]
pub struct TradeReviewStore {
    db: Arc<Db>,
}

impl TradeReviewStore {
    pub fn new(db: Arc<Db>) -> Self {
        Self { db }
    }

    /// UPSERT a trade review for `(date, account, prompt_version)`.
    /// Computes the grade server-side from `(summary, behavioral_tags)`.
    pub async fn write(
        &self,
        req: WriteTradeReviewRequest,
    ) -> Result<WriteOutcome, TradeReviewError> {
        if req.account.trim().is_empty() {
            return Err(TradeReviewError::EmptyAccount);
        }
        if req.narrative_md.trim().is_empty() {
            return Err(TradeReviewError::EmptyNarrative);
        }

        let grade = compute_grade(&req.summary, &req.behavioral_tags);
        let now = Utc::now();

        let date_str = req.date.to_string();
        let account = req.account.clone();
        let prompt_version = req.prompt_version;
        let generated_at_str = now.to_rfc3339();
        let grade_str = grade.grade.as_str().to_string();
        let grade_score = grade.score;
        let summary_json = serde_json::to_string(&req.summary)?;
        let tags_json = serde_json::to_string(&req.behavioral_tags)?;
        let observations_json = serde_json::to_string(&req.leg_observations)?;
        let narrative = req.narrative_md.clone();
        let llm_call_id = req.llm_call_id.clone();
        let summary_for_row = req.summary.clone();
        let tags_for_row = req.behavioral_tags.clone();
        let observations_for_row = req.leg_observations.clone();

        self.db
            .with_conn(move |conn| {
                conn.execute(
                    "INSERT INTO day_reviews (
                        date, account, prompt_version, generated_at, grade, grade_score,
                        gross_pnl, net_pnl, commissions_total, n_round_trips, n_carryover,
                        win_rate, behavioral_tags, leg_observations, summary_json,
                        narrative_md, llm_call_id
                     ) VALUES (
                        ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                        ?16, ?17
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
                        llm_call_id       = excluded.llm_call_id",
                    params![
                        date_str,
                        account,
                        prompt_version,
                        generated_at_str,
                        grade_str,
                        grade_score,
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
                grade: grade.grade,
                grade_score: grade.score,
                summary: summary_for_row_owned(&summary_for_row),
                behavioral_tags: tags_for_row,
                leg_observations: observations_for_row,
                narrative_md: req.narrative_md,
                llm_call_id: req.llm_call_id,
            },
            grade,
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
                    "SELECT date, account, prompt_version, generated_at, grade, grade_score,
                            gross_pnl, net_pnl, commissions_total, n_round_trips, n_carryover,
                            win_rate, behavioral_tags, leg_observations, summary_json,
                            narrative_md, llm_call_id
                     FROM day_reviews
                     WHERE date = ?1 AND account = ?2 AND prompt_version = ?3",
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
                    "SELECT date, account, prompt_version, generated_at, grade, grade_score,
                            gross_pnl, net_pnl, commissions_total, n_round_trips, n_carryover,
                            win_rate, behavioral_tags, leg_observations, summary_json,
                            narrative_md, llm_call_id
                     FROM day_reviews
                     WHERE date = ?1 AND account = ?2
                     ORDER BY prompt_version DESC
                     LIMIT 1",
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

fn summary_for_row_owned(s: &LegSummary) -> LegSummary {
    s.clone()
}

struct RawRow {
    date: String,
    account: String,
    prompt_version: i32,
    generated_at: String,
    grade: String,
    grade_score: f64,
    win_rate: Option<f64>,
    behavioral_tags: String,
    leg_observations: String,
    summary_json: String,
    narrative_md: String,
    llm_call_id: Option<String>,
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
    let grade = GradeLetter::parse(&raw.grade)
        .ok_or_else(|| TradeReviewError::InvalidRow(format!("grade `{}`", raw.grade)))?;
    let summary: LegSummary = serde_json::from_str(&raw.summary_json)?;
    let mut summary = summary;
    if summary.win_rate.is_none() {
        summary.win_rate = raw.win_rate;
    }
    let behavioral_tags: Vec<BehavioralTag> = serde_json::from_str(&raw.behavioral_tags)?;
    let leg_observations: Vec<LegObservation> = serde_json::from_str(&raw.leg_observations)?;
    Ok(TradeReview {
        date,
        account: raw.account,
        prompt_version: raw.prompt_version,
        generated_at,
        grade,
        grade_score: raw.grade_score,
        summary,
        behavioral_tags,
        leg_observations,
        narrative_md: raw.narrative_md,
        llm_call_id: raw.llm_call_id,
    })
}
