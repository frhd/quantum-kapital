//! Phase 4 orchestrator: turns a day of fills + behavioural tags into
//! the persisted v2 fields (`score_v2`, `discipline_v2`,
//! `risk_metrics_json`, `equity_curve_json`).
//!
//! The MCP `write_trade_review` tool and the in-app
//! [`super::TradeReviewGenerator`] both call [`compute_v2_fields`] —
//! it is the single source of truth for "what does Phase 4 add to a
//! day_reviews row."
//!
//! Per-leg realized R is reconstructed from the linked setup's
//! `dollar_risk_cents` (Phase 1 sizing column). Legs without a
//! setup-id linkage or with NULL dollar-risk surface as
//! `n_legs_unattributed` so the reviewer sees "your wiring has gaps"
//! rather than getting a phantom 0.

use std::sync::Arc;

use chrono::NaiveDate;
use rusqlite::params;

use super::attribution::LegWithR;
use super::equity_curve::reconstruct_daily_equity;
use super::grade::{compute_score_v2, ConvictionCalibration};
use super::risk_metrics::{compute_risk_metrics, DEFAULT_RISK_FREE_RATE_ANNUAL};
use super::tags::BehavioralTag;
use super::types::ReviewV2Fields;
use crate::mcp::tools::executions::ExecutionRow;
use crate::services::trade_legs::{match_legs, TradeLeg};
use crate::storage::Db;

/// Inputs to [`compute_v2_fields`].
pub struct V2ComputeInputs<'a> {
    pub date: NaiveDate,
    pub account: &'a str,
    pub fills: &'a [ExecutionRow],
    pub tags: &'a [BehavioralTag],
}

/// Compute v2 fields for a single (date, account) day. Reads the
/// `setups` table to recover per-leg dollar-risk + conviction-grade.
pub async fn compute_v2_fields(
    db: &Arc<Db>,
    inputs: V2ComputeInputs<'_>,
) -> Result<ReviewV2Fields, crate::storage::error::StorageError> {
    let legs = match_legs(inputs.fills);
    let setup_meta = fetch_setup_meta(db, &legs).await?;

    // Build per-leg LegWithR.
    let mut owned_legs: Vec<LegWithR<'_>> = Vec::with_capacity(legs.len());
    for leg in &legs {
        let realized_r = leg
            .setup_id
            .and_then(|sid| setup_meta.iter().find(|m| m.setup_id == sid))
            .and_then(|m| m.dollar_risk.filter(|dr| *dr > 1e-9))
            .map(|dr| leg.net_pnl / dr);
        owned_legs.push(LegWithR {
            leg,
            realized_r,
        });
    }

    let calibration = ConvictionCalibration::fallback();
    let conviction_lookup = |w: &LegWithR<'_>| -> Option<String> {
        w.leg
            .setup_id
            .and_then(|sid| setup_meta.iter().find(|m| m.setup_id == sid))
            .and_then(|m| m.conviction_grade.clone())
    };
    let scored = compute_score_v2(&owned_legs, &calibration, conviction_lookup, inputs.tags);

    // Equity curve over this day's fills only. Multi-day rolling is
    // recomputed by `trade_review_get_equity_curve` at read time.
    let equity_curve = reconstruct_daily_equity(inputs.fills, 0.0);

    let r_series: Vec<f64> = owned_legs
        .iter()
        .filter_map(|w| w.realized_r)
        .collect();
    let metrics =
        compute_risk_metrics(&equity_curve, &r_series, DEFAULT_RISK_FREE_RATE_ANNUAL);

    let _ = inputs.date; // unused but kept in the API for symmetry
    let _ = inputs.account;
    Ok(ReviewV2Fields {
        score_v2: Some(scored.score_v2),
        discipline_v2: Some(scored.discipline_v2),
        risk_metrics: Some(metrics),
        equity_curve: Some(equity_curve),
        formula_version: scored.formula_version,
    })
}

#[derive(Debug, Clone)]
struct SetupMeta {
    setup_id: i64,
    dollar_risk: Option<f64>,
    conviction_grade: Option<String>,
}

async fn fetch_setup_meta(
    db: &Arc<Db>,
    legs: &[TradeLeg],
) -> Result<Vec<SetupMeta>, crate::storage::error::StorageError> {
    let ids: Vec<i64> = legs
        .iter()
        .filter_map(|l| l.setup_id)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    if ids.is_empty() {
        return Ok(vec![]);
    }
    db.with_conn(move |conn| {
        let mut out: Vec<SetupMeta> = Vec::with_capacity(ids.len());
        // SQLite has no ANY() — bind one at a time.
        let mut stmt = conn.prepare(
            "SELECT id, dollar_risk_cents, conviction_grade
                 FROM setups WHERE id = ?1",
        )?;
        for id in ids {
            let row = stmt
                .query_row(params![id], |row| {
                    let id: i64 = row.get(0)?;
                    let cents: Option<i64> = row.get(1)?;
                    let conv: Option<String> = row.get(2)?;
                    Ok(SetupMeta {
                        setup_id: id,
                        dollar_risk: cents.map(|c| c as f64 / 100.0),
                        conviction_grade: conv,
                    })
                })
                .ok();
            if let Some(r) = row {
                out.push(r);
            }
        }
        Ok(out)
    })
    .await
}
