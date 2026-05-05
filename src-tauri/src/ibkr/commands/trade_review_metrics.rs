//! Phase 4 (quant-decisions): Tauri commands for the FE risk-metrics
//! surface.
//!
//! Three commands; all read-only over a date range:
//! - `trade_review_get_metrics`         → `RiskMetrics` rolled up
//!   across the range
//! - `trade_review_get_strategy_rollup` → per-strategy attribution
//! - `trade_review_get_equity_curve`    → daily equity series
//!
//! Each command pulls executions via the persisted `ExecutionsStore`
//! (so past-day reads work — the live `reqExecutions` IBKR endpoint is
//! today-only) and reuses the pure modules in
//! `services/trade_reviews/{equity_curve,risk_metrics,attribution}`.

use std::sync::Arc;

use chrono::{Duration, NaiveDate};
use rusqlite::params;
use tauri::State;

use crate::mcp::ibkr_seam::AccountReader;
use crate::mcp::tools::executions::ExecutionRow;
use crate::mcp::tools::resolve_account;
use crate::services::executions::ExecutionsStore;
use crate::services::trade_legs::{match_legs, TradeLeg};
use crate::services::trade_reviews::{
    compute_risk_metrics, reconstruct_daily_equity, rollup_by_strategy, EquityPoint, LegWithR,
    RiskMetrics, StrategyRollup, DEFAULT_RISK_FREE_RATE_ANNUAL,
};
use crate::storage::Db;

const MAX_RANGE_DAYS: i64 = 365;

fn parse_range(start: &str, end: &str) -> Result<(NaiveDate, NaiveDate), String> {
    let s = NaiveDate::parse_from_str(start, "%Y-%m-%d")
        .map_err(|e| format!("invalid start `{start}`, expected YYYY-MM-DD: {e}"))?;
    let e = NaiveDate::parse_from_str(end, "%Y-%m-%d")
        .map_err(|e| format!("invalid end `{end}`, expected YYYY-MM-DD: {e}"))?;
    if e < s {
        return Err(format!("end < start: {end} < {start}"));
    }
    if (e - s).num_days() > MAX_RANGE_DAYS {
        return Err(format!(
            "range > {MAX_RANGE_DAYS} days; chunk into smaller queries",
        ));
    }
    Ok((s, e))
}

async fn fetch_executions_in_range(
    db: &Arc<Db>,
    account: &str,
    start: NaiveDate,
    end: NaiveDate,
) -> Result<Vec<ExecutionRow>, String> {
    let store = ExecutionsStore::new(Arc::clone(db));
    let mut all = Vec::new();
    let mut day = start;
    while day <= end {
        let rows = store
            .query_with_linkage(account, day)
            .await
            .map_err(|e| e.to_string())?;
        all.extend(rows);
        day += Duration::days(1);
    }
    Ok(all)
}

/// Read per-setup `dollar_risk_cents` for the legs' linked setups.
/// Returns a `(setup_id → dollar_risk)` map. Legs without a setup
/// linkage or with NULL `dollar_risk_cents` produce no entry.
async fn fetch_dollar_risks(
    db: &Arc<Db>,
    legs: &[TradeLeg],
) -> Result<std::collections::HashMap<i64, f64>, String> {
    let ids: Vec<i64> = legs
        .iter()
        .filter_map(|l| l.setup_id)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    if ids.is_empty() {
        return Ok(Default::default());
    }
    db.with_conn(move |conn| {
        let mut stmt = conn.prepare("SELECT id, dollar_risk_cents FROM setups WHERE id = ?1")?;
        let mut out: std::collections::HashMap<i64, f64> = Default::default();
        for id in ids {
            if let Ok((id, cents)) = stmt.query_row(params![id], |row| {
                let id: i64 = row.get(0)?;
                let cents: Option<i64> = row.get(1)?;
                Ok((id, cents))
            }) {
                if let Some(c) = cents.filter(|c| *c > 0) {
                    out.insert(id, c as f64 / 100.0);
                }
            }
        }
        Ok(out)
    })
    .await
    .map_err(|e: crate::storage::error::StorageError| e.to_string())
}

#[tauri::command]
pub async fn trade_review_get_metrics(
    reader: State<'_, Arc<dyn AccountReader>>,
    db: State<'_, Arc<Db>>,
    start: String,
    end: String,
    account: Option<String>,
) -> Result<RiskMetrics, String> {
    let (s, e) = parse_range(&start, &end)?;
    let resolved = resolve_account(reader.inner().as_ref(), account.as_deref()).await?;
    let fills = fetch_executions_in_range(db.inner(), &resolved, s, e).await?;
    let curve = reconstruct_daily_equity(&fills, 0.0);
    let legs = match_legs(&fills);
    let dollar_risks = fetch_dollar_risks(db.inner(), &legs).await?;
    let r_series: Vec<f64> = legs
        .iter()
        .filter_map(|leg| {
            leg.setup_id
                .and_then(|sid| dollar_risks.get(&sid))
                .map(|dr| leg.net_pnl / dr)
        })
        .collect();
    Ok(compute_risk_metrics(
        &curve,
        &r_series,
        DEFAULT_RISK_FREE_RATE_ANNUAL,
    ))
}

#[tauri::command]
pub async fn trade_review_get_equity_curve(
    reader: State<'_, Arc<dyn AccountReader>>,
    db: State<'_, Arc<Db>>,
    start: String,
    end: String,
    account: Option<String>,
    starting_equity: Option<f64>,
) -> Result<Vec<EquityPoint>, String> {
    let (s, e) = parse_range(&start, &end)?;
    let resolved = resolve_account(reader.inner().as_ref(), account.as_deref()).await?;
    let fills = fetch_executions_in_range(db.inner(), &resolved, s, e).await?;
    Ok(reconstruct_daily_equity(
        &fills,
        starting_equity.unwrap_or(0.0),
    ))
}

#[tauri::command]
pub async fn trade_review_get_strategy_rollup(
    reader: State<'_, Arc<dyn AccountReader>>,
    db: State<'_, Arc<Db>>,
    start: String,
    end: String,
    account: Option<String>,
) -> Result<Vec<StrategyRollup>, String> {
    let (s, e) = parse_range(&start, &end)?;
    let resolved = resolve_account(reader.inner().as_ref(), account.as_deref()).await?;
    let fills = fetch_executions_in_range(db.inner(), &resolved, s, e).await?;
    let legs = match_legs(&fills);
    let dollar_risks = fetch_dollar_risks(db.inner(), &legs).await?;
    let with_r: Vec<LegWithR<'_>> = legs
        .iter()
        .map(|leg| LegWithR {
            leg,
            realized_r: leg
                .setup_id
                .and_then(|sid| dollar_risks.get(&sid))
                .map(|dr| leg.net_pnl / dr),
        })
        .collect();
    Ok(rollup_by_strategy(&with_r))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ibkr::types::{ExecutionSide, IbkrExecution};
    use crate::mcp::tools::test_support::make_db;
    use chrono::{Datelike, TimeZone, Utc};

    fn fill(date: NaiveDate, hour: u32, realized: f64, commission: f64) -> IbkrExecution {
        let utc_dt = Utc
            .with_ymd_and_hms(date.year(), date.month(), date.day(), hour, 0, 0)
            .single()
            .unwrap();
        IbkrExecution {
            exec_id: format!("e-{date}-{hour}"),
            account: "U1".into(),
            symbol: "AAPL".into(),
            contract_type: "STK".into(),
            expiry: None,
            strike: None,
            right: None,
            multiplier: None,
            side: ExecutionSide::Sold,
            qty: 100.0,
            avg_price: 200.0,
            currency: Some("USD".into()),
            exec_time: utc_dt,
            order_id: 1,
            commission: Some(commission),
            realized_pnl: Some(realized),
            commission_currency: Some("USD".into()),
        }
    }

    #[tokio::test]
    async fn parse_range_rejects_inverted() {
        assert!(parse_range("2026-05-10", "2026-05-04").is_err());
    }

    #[tokio::test]
    async fn parse_range_rejects_too_wide() {
        assert!(parse_range("2025-01-01", "2026-05-10").is_err());
    }

    #[tokio::test]
    async fn parse_range_accepts_inclusive_window() {
        let (s, e) = parse_range("2026-05-04", "2026-05-10").unwrap();
        assert_eq!((e - s).num_days(), 6);
    }

    #[tokio::test]
    async fn equity_curve_round_trips_persisted_fills() {
        let (_tmp, db) = make_db();
        let store = ExecutionsStore::new(Arc::clone(&db));
        let d1 = NaiveDate::from_ymd_opt(2026, 5, 4).unwrap();
        let d2 = NaiveDate::from_ymd_opt(2026, 5, 5).unwrap();
        let d3 = NaiveDate::from_ymd_opt(2026, 5, 6).unwrap();
        let rows = vec![
            fill(d1, 14, 200.0, 1.0),
            fill(d2, 14, -100.0, 1.0),
            fill(d3, 14, 50.0, 0.5),
        ];
        store.record(&rows).await.unwrap();
        let fills = fetch_executions_in_range(&db, "U1", d1, d3).await.unwrap();
        let pts = reconstruct_daily_equity(&fills, 100_000.0);
        assert_eq!(pts.len(), 3);
        assert!((pts[0].equity - 100_199.0).abs() < 1e-9);
        assert!((pts[2].equity - 100_147.5).abs() < 1e-9);
    }

    #[tokio::test]
    async fn metrics_over_30_days_passes_sharpe_floor() {
        let (_tmp, db) = make_db();
        let store = ExecutionsStore::new(Arc::clone(&db));
        let start = NaiveDate::from_ymd_opt(2026, 4, 1).unwrap();
        // 30 trading-day-ish series of small alternating gains/losses.
        let mut rows = Vec::new();
        for i in 0..30 {
            let d = start + Duration::days(i);
            // slight positive drift
            let realized = if i % 4 == 0 { -50.0 } else { 75.0 };
            rows.push(fill(d, 14, realized, 0.5));
        }
        store.record(&rows).await.unwrap();
        let fills = fetch_executions_in_range(&db, "U1", start, start + Duration::days(29))
            .await
            .unwrap();
        let curve = reconstruct_daily_equity(&fills, 100_000.0);
        assert_eq!(curve.len(), 30);
        let metrics = compute_risk_metrics(&curve, &[], DEFAULT_RISK_FREE_RATE_ANNUAL);
        assert!(
            metrics.sharpe.is_some(),
            "30 daily samples must satisfy the N=20 floor",
        );
        assert_eq!(metrics.n_days, 30);
    }
}
