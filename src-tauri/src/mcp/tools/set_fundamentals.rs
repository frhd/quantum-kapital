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
        if let Some(msg) = surprising_change(
            "sharesOutstanding",
            p.shares_outstanding,
            c.shares_outstanding,
        ) {
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
    use std::sync::atomic::{AtomicUsize, Ordering};

    use async_trait::async_trait;

    use super::*;

    use crate::events::EventEmitter;
    use crate::ibkr::types::{
        AnalystEstimate, AnalystEstimates, CurrentMetrics, FundamentalData as Fd,
        HistoricalFinancial,
    };
    use crate::mcp::tools::fundamentals::GetFundamentalsArgs;
    use crate::mcp::tools::test_support::{handler_for_db, make_db, NotConnectedStub};
    use crate::services::auto_scanner::AutoScannerService;
    use crate::services::candidate_promoter::CandidatePromoter;
    use crate::services::candidate_universe::CandidateUniverseService;
    use crate::services::financial_data_service::FinancialDataService;
    use crate::services::fundamentals_provider::composite::CompositeFundamentalsProvider;
    use crate::services::fundamentals_provider::manual::ManualFundamentalsProvider;
    use crate::services::fundamentals_provider::{FundamentalsError, FundamentalsProvider};
    use crate::services::historical_data_service::{HistoricalDataFetcher, HistoricalDataService};
    use crate::services::llm_service::LlmService;
    use crate::services::manual_fundamentals_store::ManualFundamentalsStore;
    use crate::services::quote_service::QuoteService;
    use crate::services::social_sentiment::SocialSentimentService;
    use crate::services::tracker_service::TrackerService;

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
        assert!(body["isNew"].as_bool().unwrap());
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
        assert!(!body["isNew"].as_bool().unwrap());
        assert_eq!(body["diff"]["kind"].as_str().unwrap(), "update");
        assert_eq!(
            body["diff"]["changed"]["peRatio"]["from"].as_f64().unwrap(),
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
        let set_count = audits
            .iter()
            .filter(|a| a.tool == "set_fundamentals")
            .count();
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

    /// Counts every `fetch` call on the AV layer of the composite. The
    /// tracer-bullet test asserts this counter stays at zero so a manual
    /// write definitively short-circuits the AV path.
    struct CountingAvProvider {
        calls: Arc<AtomicUsize>,
    }

    impl CountingAvProvider {
        fn new(calls: Arc<AtomicUsize>) -> Self {
            Self { calls }
        }
    }

    #[async_trait]
    impl FundamentalsProvider for CountingAvProvider {
        async fn fetch(&self, symbol: &str) -> Result<Fd, FundamentalsError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Err(FundamentalsError::NotFound(symbol.to_string()))
        }
    }

    /// Build an `McpHandler` whose `fundamentals_provider` is the
    /// production-shape composite (manual → AV) but whose AV layer is a
    /// counting fake. Mirrors `test_support::build_handler` — copied
    /// inline because that helper hard-codes a `FakeFundamentalsProvider`
    /// which makes the tracer-bullet assertion (zero AV calls) trivial.
    fn handler_with_composite(
        db: Arc<crate::storage::Db>,
    ) -> (McpHandler, Arc<ManualFundamentalsStore>, Arc<AtomicUsize>) {
        use crate::mcp::ibkr_seam::AccountReader;
        use crate::middleware::HistoricalRateLimiter;
        use crate::services::auto_scanner::MarketScanner;

        let store = Arc::new(ManualFundamentalsStore::new(Arc::clone(&db)));
        let manual = Arc::new(ManualFundamentalsProvider::new(Arc::clone(&store)));
        let calls = Arc::new(AtomicUsize::new(0));
        let av: Arc<dyn FundamentalsProvider> =
            Arc::new(CountingAvProvider::new(Arc::clone(&calls)));
        let composite: Arc<dyn FundamentalsProvider> =
            Arc::new(CompositeFundamentalsProvider::new(Arc::clone(&manual), av));

        let llm = Arc::new(LlmService::new(
            "test-key".to_string(),
            Arc::clone(&db),
            100.0,
        ));
        let tracker = Arc::new(TrackerService::new(Arc::clone(&db)));
        let financial = Arc::new(FinancialDataService::new(String::new()).with_db(Arc::clone(&db)));
        let stub = Arc::new(NotConnectedStub);
        struct Panicker;
        #[async_trait]
        impl HistoricalDataFetcher for Panicker {
            async fn fetch_historical(
                &self,
                _r: crate::ibkr::types::historical::HistoricalDataRequest,
            ) -> crate::ibkr::error::Result<Vec<crate::ibkr::types::historical::HistoricalBar>>
            {
                panic!("historical fetch attempted in tracer-bullet test")
            }
        }
        let fetcher: Arc<dyn HistoricalDataFetcher> = Arc::new(Panicker);
        let hist = Arc::new(HistoricalDataService::new(
            Arc::clone(&db),
            fetcher,
            Arc::new(HistoricalRateLimiter::new(60)),
        ));
        let quote_fetcher: Arc<dyn crate::services::quote_service::QuoteFetcher> =
            Arc::clone(&stub) as _;
        let quote = Arc::new(QuoteService::new(quote_fetcher));
        let ibkr_client: Arc<dyn AccountReader> = Arc::clone(&stub) as _;
        let market_scanner: Arc<dyn MarketScanner> = Arc::clone(&stub) as _;
        let candidates = Arc::new(CandidateUniverseService::new(Arc::clone(&db)));
        let promoter = Arc::new(CandidatePromoter::new(
            Arc::clone(&candidates),
            Arc::clone(&tracker),
            0.0,
        ));
        let auto_scanner = Arc::new(AutoScannerService::new(
            Arc::clone(&market_scanner),
            Arc::clone(&tracker),
            Arc::clone(&promoter),
            Arc::clone(&db),
            crate::config::settings::AutoScannerConfig::default(),
        ));
        let emitter = Arc::new(EventEmitter::for_capture());
        let social = Arc::new(SocialSentimentService::new(Arc::clone(&db), Vec::new()));
        let news_provider: Arc<dyn crate::services::news_provider::NewsProvider> =
            Arc::new(crate::services::news_provider::test_support::FakeNewsProvider::new());

        let handler = McpHandler::new(
            llm,
            tracker,
            db,
            financial,
            composite,
            Arc::clone(&store),
            news_provider,
            hist,
            quote,
            ibkr_client,
            auto_scanner,
            market_scanner,
            emitter,
            social,
            candidates,
            promoter,
            "interactive".to_string(),
        );
        (handler, store, calls)
    }

    /// Tracer-bullet — the master plan's cross-phase verification §1:
    /// `set_fundamentals` then `get_fundamentals` round-trips through the
    /// composite without ever touching the AV layer. Assertion is
    /// "AV calls counter == 0" after both tool invocations.
    #[tokio::test]
    async fn tracer_bullet_set_then_get_round_trips_without_hitting_av() {
        let (_tmp, db) = make_db();
        let (handler, _store, av_calls) = handler_with_composite(db);

        let payload = SetFundamentalsArgs {
            symbol: "AAPL".to_string(),
            as_of_date: "2026-05-02".to_string(),
            source: "tracer paste".to_string(),
            notes: Some("tracer test".to_string()),
            historical: vec![HistoricalFinancial {
                year: 2024,
                revenue: 391.0,
                net_income: 99.8,
                eps: 6.5,
            }],
            analyst_estimates: Some(AnalystEstimates {
                revenue: vec![AnalystEstimate {
                    year: 2025,
                    estimate: 420.0,
                }],
                eps: vec![AnalystEstimate {
                    year: 2025,
                    estimate: 7.1,
                }],
            }),
            current_metrics: CurrentMetrics {
                price: Some(192.5),
                pe_ratio: 30.0,
                shares_outstanding: 15_500.0,
                name: Some("Apple Inc.".into()),
                exchange: Some("NASDAQ".into()),
                market_cap: Some("3000000000000".into()),
                dividend_yield: Some(0.005),
            },
        };
        let r = handler
            .set_fundamentals(Parameters(payload))
            .await
            .expect("rmcp Ok");
        assert_eq!(r.is_error, Some(false), "set_fundamentals must succeed");

        // Now read it back through the composite via get_fundamentals.
        let g = handler
            .get_fundamentals(Parameters(GetFundamentalsArgs {
                symbol: "aapl".to_string(),
            }))
            .await
            .expect("rmcp Ok");
        assert_eq!(g.is_error, Some(false), "get_fundamentals must succeed");
        let body = g.structured_content.expect("structured");
        assert_eq!(body["symbol"].as_str().unwrap(), "AAPL");
        assert_eq!(body["currentMetrics"]["peRatio"].as_f64().unwrap(), 30.0);
        assert_eq!(
            body["currentMetrics"]["sharesOutstanding"]
                .as_f64()
                .unwrap(),
            15_500.0
        );
        assert_eq!(body["historical"].as_array().unwrap().len(), 1);

        // Hard Invariant #7 / #8: zero AV calls fired across the whole flow.
        assert_eq!(
            av_calls.load(Ordering::SeqCst),
            0,
            "manual store must short-circuit the AV layer"
        );
    }

    /// Manual-write-invalidates-AV-cache — the master plan's exit
    /// criterion. Pre-populate the AV file cache for AAPL with the
    /// production cache-key suffixes (`overview` / `income_statement`
    /// / `earnings` — the divergence with the older `_income` suffix
    /// was a Phase-5 fix), write a manual row, assert the AV cache
    /// rows for AAPL are gone.
    #[tokio::test]
    async fn manual_write_clears_av_file_cache_for_symbol() {
        use crate::services::cache_service::CacheService;
        use serde_json::json;
        use tempfile::TempDir;

        let cache_dir = TempDir::new().unwrap();
        let cache = CacheService::new(cache_dir.path()).unwrap();
        // Seed all three AV cache keys for AAPL with arbitrary payloads.
        cache
            .write("AAPL_overview", &json!({"sentinel": "ov"}))
            .unwrap();
        cache
            .write("AAPL_income_statement", &json!({"sentinel": "in"}))
            .unwrap();
        cache
            .write("AAPL_earnings", &json!({"sentinel": "ea"}))
            .unwrap();

        let financial = FinancialDataService::with_cache_dir(String::new(), cache_dir.path());
        // Sanity: rows are present.
        assert!(financial.cache_for_test().is_valid("AAPL_overview"));
        assert!(financial.cache_for_test().is_valid("AAPL_income_statement"));
        assert!(financial.cache_for_test().is_valid("AAPL_earnings"));

        // Drive the same invalidation the MCP tool uses.
        financial.clear_fundamentals_cache("aapl");
        assert!(!financial.cache_for_test().is_valid("AAPL_overview"));
        assert!(!financial.cache_for_test().is_valid("AAPL_income_statement"));
        assert!(!financial.cache_for_test().is_valid("AAPL_earnings"));
    }
}
