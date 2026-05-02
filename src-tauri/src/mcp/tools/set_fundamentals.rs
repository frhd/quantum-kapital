//! `set_fundamentals` — manual fundamentals write tool.
//!
//! Phase 4 of the AV strip-out (`loop/plan/master.md`). The IBKR
//! fundamentals migration was abandoned 2026-05-02 (deprecated API +
//! missing entitlement), so the operator-curated manual path is the
//! primary source going forward; AV survives as an opportunistic
//! fallback only.
//!
//! ## Surface
//!
//! - **Input:** envelope (`symbol`, `asOfDate`, `source`, optional
//!   `notes`) flattened with the `FundamentalData` shape (`historical`,
//!   `analystEstimates`, `currentMetrics`).
//! - **Validation:** symbol matches `^[A-Z][A-Z0-9.\-]{0,9}$`, ISO 8601
//!   date, non-empty source, finite numeric fields, `peRatio >= 0`,
//!   `sharesOutstanding > 0`. Out-of-range or 5x changes vs. prior emit
//!   a `warnings` field but do not reject the write — the operator is
//!   the authority of last resort.
//! - **Side effects:** persists to `manual_fundamentals`, invalidates
//!   the AV file-cache rows for that symbol (Hard Invariant #8), audits
//!   via `services::mcp_audit`, emits `FundamentalsManualWritten`.
//! - **Surveillance contract:** writes operator-curated reference data,
//!   never market actions. The MCP surface remains read-only-plus-
//!   acknowledgments for order-affecting operations (workspace
//!   `CLAUDE.md`).

use std::sync::Arc;

use chrono::Utc;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{model::CallToolResult, tool, tool_router, ErrorData as McpError};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::events::AppEvent;
use crate::ibkr::types::{AnalystEstimates, CurrentMetrics, FundamentalData, HistoricalFinancial};
use crate::mcp::handler::McpHandler;
use crate::mcp::tools::map_tool_result;
use crate::mcp::tools::write_support::{emit_event, record_audit, stamp_audit_summary};
use crate::services::manual_fundamentals_store::ManualFundamentalsRow;

/// Maximum allowed symbol length. Matches the regex bound in [`validate_symbol`].
const MAX_SYMBOL_LEN: usize = 10;

/// Threshold above which a numeric change vs. the prior write surfaces
/// a `warnings` entry in the tool response. 5x mirrors the
/// "surprising movement" sanity heuristic from `master.md` § "Open risks".
const SURPRISING_CHANGE_RATIO: f64 = 5.0;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetFundamentalsArgs {
    /// Ticker symbol. Uppercased before persistence; must match
    /// `^[A-Z][A-Z0-9.\-]{0,9}$`.
    pub symbol: String,
    /// ISO 8601 date the snapshot is "as of" (e.g., `2026-05-02`).
    pub as_of_date: String,
    /// Free-form provenance string (e.g., `"Bloomberg terminal paste"`,
    /// `"Yahoo Finance 2026-05-02"`). Required so the audit row records
    /// where the data came from.
    pub source: String,
    /// Optional operator notes preserved on the audit row's
    /// `result_summary` (not persisted in the fundamentals payload).
    #[serde(default)]
    pub notes: Option<String>,
    /// Historical financials, oldest-to-newest or newest-to-oldest —
    /// preserved verbatim. May be empty when the operator only has
    /// current-snapshot data.
    #[serde(default)]
    pub historical: Vec<HistoricalFinancial>,
    /// Optional analyst-estimate vectors. Omit when the operator has
    /// no estimates (typical for small caps, ADRs).
    #[serde(default)]
    pub analyst_estimates: Option<AnalystEstimates>,
    /// Current valuation snapshot. Required.
    pub current_metrics: CurrentMetrics,
}

#[tool_router(router = set_fundamentals_router, vis = "pub(crate)")]
impl McpHandler {
    #[tool(
        name = "set_fundamentals",
        description = "Persist operator-curated fundamentals for `symbol` into the manual store. The composite provider reads this row before any Alpha Vantage cache or live AV call, so subsequent `get_fundamentals(symbol)` reflects the new payload immediately. Validates symbol shape, ISO 8601 `asOfDate`, non-empty `source`, and finite numerics; surprising changes vs. the prior row (5x movements, sign flips) surface in `warnings` rather than rejecting the write. Returns `{ symbol, asOfDate, source, isNew, priorWrittenAt, warnings, diff }`. Surveillance-only: writes reference data, never orders."
    )]
    pub async fn set_fundamentals(
        &self,
        Parameters(args): Parameters<SetFundamentalsArgs>,
    ) -> Result<CallToolResult, McpError> {
        let SetFundamentalsArgs {
            symbol,
            as_of_date,
            source,
            notes,
            historical,
            analyst_estimates,
            current_metrics,
        } = args;

        let symbol = match validate_symbol(&symbol) {
            Ok(s) => s,
            Err(msg) => return map_tool_result::<(), String>(Err(msg)),
        };
        if let Err(msg) = validate_as_of_date(&as_of_date) {
            return map_tool_result::<(), String>(Err(msg));
        }
        let source = source.trim().to_string();
        if source.is_empty() {
            return map_tool_result::<(), String>(Err("source must not be blank".to_string()));
        }
        if let Err(msg) = validate_metrics(&current_metrics) {
            return map_tool_result::<(), String>(Err(msg));
        }
        if let Err(msg) = validate_historical(&historical) {
            return map_tool_result::<(), String>(Err(msg));
        }

        let payload = FundamentalData {
            symbol: symbol.clone(),
            historical,
            analyst_estimates,
            current_metrics,
        };

        // Audit BEFORE the mutation so an aborted write still leaves a
        // visible row (mirrors `ack_alert` and `write_research_note`).
        let input_for_audit = json!({
            "symbol": symbol,
            "as_of_date": as_of_date,
            "source": source,
            "has_notes": notes.as_deref().map(|s| !s.trim().is_empty()).unwrap_or(false),
            "historical_len": payload.historical.len(),
            "has_analyst_estimates": payload.analyst_estimates.is_some(),
        });
        let audit_id = match record_audit(
            &self.db,
            "set_fundamentals",
            &input_for_audit,
            &self.caller,
        )
        .await
        {
            Ok(id) => id,
            Err(e) => return map_tool_result::<(), String>(Err(e)),
        };

        let written_at = Utc::now().timestamp();
        let upsert = match self
            .manual_fundamentals
            .upsert(
                &symbol,
                payload.clone(),
                &as_of_date,
                &source,
                &self.caller,
                written_at,
            )
            .await
        {
            Ok(o) => o,
            Err(e) => {
                return map_tool_result::<(), String>(Err(format!(
                    "manual fundamentals upsert failed: {e}"
                )))
            }
        };

        // Hard Invariant #8: manual write invalidates the AV cache for
        // this symbol so a future store-clear can never resurface
        // pre-manual data.
        self.financial_service.clear_fundamentals_cache(&symbol);

        let diff = build_diff(&upsert.prior, &upsert.current);
        let warnings = build_warnings(&upsert.prior, &upsert.current);
        let is_new = upsert.prior.is_none();
        let prior_written_at = upsert.prior.as_ref().map(|p| p.written_at);

        let summary = format!(
            "symbol={}, as_of_date={}, is_new={}, warnings={}",
            symbol,
            as_of_date,
            is_new,
            warnings.len(),
        );
        stamp_audit_summary(&self.db, audit_id, &summary).await;
        emit_event(
            &self.emitter,
            AppEvent::FundamentalsManualWritten {
                symbol: symbol.clone(),
                as_of_date: as_of_date.clone(),
                source: source.clone(),
            },
        )
        .await;

        let result = json!({
            "symbol": symbol,
            "asOfDate": as_of_date,
            "source": source,
            "isNew": is_new,
            "priorWrittenAt": prior_written_at,
            "writtenAt": written_at,
            "warnings": warnings,
            "diff": diff,
        });
        map_tool_result::<_, String>(Ok(result))
    }
}

fn validate_symbol(symbol: &str) -> Result<String, String> {
    let trimmed = symbol.trim();
    if trimmed.is_empty() {
        return Err("symbol must not be empty".to_string());
    }
    let upper = trimmed.to_uppercase();
    if upper.len() > MAX_SYMBOL_LEN {
        return Err(format!(
            "symbol must be at most {MAX_SYMBOL_LEN} characters; got {}",
            upper.len()
        ));
    }
    let mut chars = upper.chars();
    let first = chars.next().expect("non-empty");
    if !first.is_ascii_uppercase() {
        return Err(format!(
            "symbol must start with an uppercase letter; got `{upper}`"
        ));
    }
    for c in chars {
        if !(c.is_ascii_uppercase() || c.is_ascii_digit() || c == '.' || c == '-') {
            return Err(format!(
                "symbol contains invalid character `{c}`; allowed: A-Z, 0-9, `.`, `-`"
            ));
        }
    }
    Ok(upper)
}

fn validate_as_of_date(s: &str) -> Result<(), String> {
    chrono::NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d")
        .map(|_| ())
        .map_err(|e| format!("asOfDate must be ISO 8601 (YYYY-MM-DD); {e}"))
}

fn validate_metrics(m: &CurrentMetrics) -> Result<(), String> {
    if !m.pe_ratio.is_finite() {
        return Err("currentMetrics.peRatio must be a finite number".to_string());
    }
    if m.pe_ratio < 0.0 {
        return Err(format!(
            "currentMetrics.peRatio must be >= 0 (negative P/E nonsensical); got {}",
            m.pe_ratio
        ));
    }
    if !m.shares_outstanding.is_finite() || m.shares_outstanding <= 0.0 {
        return Err(format!(
            "currentMetrics.sharesOutstanding must be a finite positive number; got {}",
            m.shares_outstanding
        ));
    }
    if let Some(p) = m.price {
        if !p.is_finite() {
            return Err("currentMetrics.price must be finite when present".to_string());
        }
    }
    if let Some(d) = m.dividend_yield {
        if !d.is_finite() {
            return Err("currentMetrics.dividendYield must be finite when present".to_string());
        }
    }
    Ok(())
}

fn validate_historical(rows: &[HistoricalFinancial]) -> Result<(), String> {
    for (i, h) in rows.iter().enumerate() {
        if !h.revenue.is_finite() || !h.net_income.is_finite() || !h.eps.is_finite() {
            return Err(format!(
                "historical[{i}] contains non-finite numbers (NaN/Inf disallowed)"
            ));
        }
    }
    Ok(())
}

fn build_diff(prior: &Option<ManualFundamentalsRow>, current: &ManualFundamentalsRow) -> Value {
    match prior {
        None => json!({"kind": "new"}),
        Some(prior) => {
            let mut changes = serde_json::Map::new();
            let p = &prior.data.current_metrics;
            let c = &current.data.current_metrics;
            track_change(&mut changes, "peRatio", p.pe_ratio, c.pe_ratio);
            track_change(
                &mut changes,
                "sharesOutstanding",
                p.shares_outstanding,
                c.shares_outstanding,
            );
            if let (Some(pp), Some(cp)) = (p.price, c.price) {
                track_change(&mut changes, "price", pp, cp);
            }
            if prior.as_of_date != current.as_of_date {
                changes.insert(
                    "asOfDate".into(),
                    json!({"from": prior.as_of_date, "to": current.as_of_date}),
                );
            }
            if prior.data.historical.len() != current.data.historical.len() {
                changes.insert(
                    "historicalLen".into(),
                    json!({
                        "from": prior.data.historical.len(),
                        "to": current.data.historical.len(),
                    }),
                );
            }
            json!({"kind": "update", "changed": Value::Object(changes)})
        }
    }
}

fn track_change(out: &mut serde_json::Map<String, Value>, name: &str, from: f64, to: f64) {
    if (from - to).abs() > f64::EPSILON {
        out.insert(name.into(), json!({"from": from, "to": to}));
    }
}

fn build_warnings(
    prior: &Option<ManualFundamentalsRow>,
    current: &ManualFundamentalsRow,
) -> Vec<String> {
    let mut warnings = Vec::new();
    if let Some(prior) = prior {
        let p = &prior.data.current_metrics;
        let c = &current.data.current_metrics;
        if let Some(msg) = surprising_change("peRatio", p.pe_ratio, c.pe_ratio) {
            warnings.push(msg);
        }
        if let Some(msg) =
            surprising_change("sharesOutstanding", p.shares_outstanding, c.shares_outstanding)
        {
            warnings.push(msg);
        }
        if let (Some(pp), Some(cp)) = (p.price, c.price) {
            if let Some(msg) = surprising_change("price", pp, cp) {
                warnings.push(msg);
            }
        }
    }
    warnings
}

fn surprising_change(name: &str, from: f64, to: f64) -> Option<String> {
    if !from.is_finite() || !to.is_finite() {
        return None;
    }
    if from == 0.0 && to != 0.0 {
        return Some(format!(
            "{name} went from 0 to {to} — verify the new value is correct"
        ));
    }
    if from != 0.0 && to == 0.0 {
        return Some(format!(
            "{name} went from {from} to 0 — verify the new value is correct"
        ));
    }
    if from.signum() != to.signum() && from != 0.0 && to != 0.0 {
        return Some(format!(
            "{name} flipped sign ({from} → {to}) — verify the new value is correct"
        ));
    }
    let ratio = (to.abs() / from.abs()).max(from.abs() / to.abs());
    if ratio.is_finite() && ratio >= SURPRISING_CHANGE_RATIO {
        return Some(format!(
            "{name} changed {ratio:.1}x ({from} → {to}) — verify the new value is correct"
        ));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::ibkr::types::{
        AnalystEstimate, AnalystEstimates, CurrentMetrics, HistoricalFinancial,
    };
    use crate::mcp::tools::test_support::{handler_for_db, make_db};

    fn metrics(pe: f64, shares: f64) -> CurrentMetrics {
        CurrentMetrics {
            price: None,
            pe_ratio: pe,
            shares_outstanding: shares,
            name: None,
            exchange: None,
            market_cap: None,
            dividend_yield: None,
        }
    }

    fn args(symbol: &str, pe: f64) -> SetFundamentalsArgs {
        SetFundamentalsArgs {
            symbol: symbol.to_string(),
            as_of_date: "2026-05-02".to_string(),
            source: "test paste".to_string(),
            notes: None,
            historical: vec![HistoricalFinancial {
                year: 2024,
                revenue: 100.0,
                net_income: 10.0,
                eps: 1.0,
            }],
            analyst_estimates: Some(AnalystEstimates {
                revenue: vec![AnalystEstimate {
                    year: 2025,
                    estimate: 120.0,
                }],
                eps: vec![AnalystEstimate {
                    year: 2025,
                    estimate: 1.2,
                }],
            }),
            current_metrics: metrics(pe, 1_000.0),
        }
    }

    #[tokio::test]
    async fn happy_path_persists_and_returns_is_new() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(Arc::clone(&db));
        let result = handler
            .set_fundamentals(Parameters(args("AAPL", 30.0)))
            .await
            .expect("rmcp Ok");
        assert_eq!(result.is_error, Some(false));
        let body = result.structured_content.expect("structured");
        assert_eq!(body["symbol"].as_str().unwrap(), "AAPL");
        assert_eq!(body["isNew"].as_bool().unwrap(), true);
        assert_eq!(body["diff"]["kind"].as_str().unwrap(), "new");
        assert!(body["warnings"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn second_write_returns_diff_kind_update() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(Arc::clone(&db));
        handler
            .set_fundamentals(Parameters(args("AAPL", 30.0)))
            .await
            .expect("first ok");
        let r = handler
            .set_fundamentals(Parameters(args("AAPL", 32.0)))
            .await
            .expect("second ok");
        assert_eq!(r.is_error, Some(false));
        let body = r.structured_content.expect("structured");
        assert_eq!(body["isNew"].as_bool().unwrap(), false);
        assert_eq!(body["diff"]["kind"].as_str().unwrap(), "update");
        assert_eq!(
            body["diff"]["changed"]["peRatio"]["from"]
                .as_f64()
                .unwrap(),
            30.0
        );
        assert_eq!(
            body["diff"]["changed"]["peRatio"]["to"].as_f64().unwrap(),
            32.0
        );
        assert!(body["warnings"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn surprising_change_surfaces_warning_but_still_writes() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(Arc::clone(&db));
        handler
            .set_fundamentals(Parameters(args("AAPL", 30.0)))
            .await
            .unwrap();
        let r = handler
            .set_fundamentals(Parameters(args("AAPL", 200.0))) // 6.67x change
            .await
            .expect("rmcp ok");
        assert_eq!(r.is_error, Some(false));
        let body = r.structured_content.expect("structured");
        let warnings = body["warnings"].as_array().expect("warnings");
        assert!(!warnings.is_empty(), "expected at least one warning");
        let msg = warnings[0].as_str().expect("warning is text");
        assert!(msg.contains("peRatio"), "got: {msg}");
    }

    #[tokio::test]
    async fn invalid_symbol_returns_domain_error() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(Arc::clone(&db));
        let mut a = args("AAPL", 30.0);
        a.symbol = "1bad".to_string();
        let r = handler
            .set_fundamentals(Parameters(a))
            .await
            .expect("rmcp ok");
        assert_eq!(r.is_error, Some(true));
    }

    #[tokio::test]
    async fn invalid_date_returns_domain_error() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(Arc::clone(&db));
        let mut a = args("AAPL", 30.0);
        a.as_of_date = "tomorrow".to_string();
        let r = handler
            .set_fundamentals(Parameters(a))
            .await
            .expect("rmcp ok");
        assert_eq!(r.is_error, Some(true));
    }

    #[tokio::test]
    async fn blank_source_returns_domain_error() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(Arc::clone(&db));
        let mut a = args("AAPL", 30.0);
        a.source = "   ".to_string();
        let r = handler
            .set_fundamentals(Parameters(a))
            .await
            .expect("rmcp ok");
        assert_eq!(r.is_error, Some(true));
    }

    #[tokio::test]
    async fn negative_pe_returns_domain_error() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(Arc::clone(&db));
        let r = handler
            .set_fundamentals(Parameters(args("AAPL", -5.0)))
            .await
            .expect("rmcp ok");
        assert_eq!(r.is_error, Some(true));
    }

    #[tokio::test]
    async fn zero_shares_returns_domain_error() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(Arc::clone(&db));
        let mut a = args("AAPL", 30.0);
        a.current_metrics.shares_outstanding = 0.0;
        let r = handler
            .set_fundamentals(Parameters(a))
            .await
            .expect("rmcp ok");
        assert_eq!(r.is_error, Some(true));
    }

    #[tokio::test]
    async fn audit_row_written_for_each_call() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(Arc::clone(&db));
        handler
            .set_fundamentals(Parameters(args("AAPL", 30.0)))
            .await
            .unwrap();
        handler
            .set_fundamentals(Parameters(args("MSFT", 25.0)))
            .await
            .unwrap();
        let audits = crate::services::mcp_audit::list(&handler.db, 10, 0)
            .await
            .unwrap();
        let set_count = audits.iter().filter(|a| a.tool == "set_fundamentals").count();
        assert_eq!(set_count, 2);
    }

    #[tokio::test]
    async fn validate_symbol_unit_lifts_lower_to_upper_and_rejects_numeric_first_char() {
        assert_eq!(validate_symbol("aapl").unwrap(), "AAPL");
        assert_eq!(validate_symbol("BRK.B").unwrap(), "BRK.B");
        assert!(validate_symbol("").is_err());
        assert!(validate_symbol("1tech").is_err());
        assert!(validate_symbol("toolongsymbol").is_err());
    }
}
