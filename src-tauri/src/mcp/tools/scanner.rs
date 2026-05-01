//! `run_scanner` — ad-hoc IBKR market scan via a named auto-scanner
//! profile.
//!
//! The agent supplies a `profile_name` already defined under
//! `auto_scanner.profiles` in settings; the tool maps that to the
//! profile's `ScannerSubscription` (via the existing
//! `auto_scanner::subscription_for` helper) and runs a one-shot scan
//! through the `MarketScanner` seam. Profile discovery is
//! case-sensitive and surfaces "unknown profile" as a domain error so
//! the agent can adjust without retrying blindly.
//!
//! Surveillance-only contract: this tool DOES NOT promote rows into
//! the watchlist. The auto-scanner background sweep is the only path
//! that mutates `tracked_tickers`. The agent gets the raw scanner
//! output to reason about; any persistence requires a separate (not
//! yet wired) tool.

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{model::CallToolResult, tool, tool_router, ErrorData as McpError};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::mcp::handler::McpHandler;
use crate::mcp::tools::map_tool_result;
use crate::services::auto_scanner::subscription_for;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RunScannerArgs {
    /// Name of a `ScanProfile` configured under
    /// `auto_scanner.profiles` in settings. Case-sensitive.
    pub profile_name: String,
}

#[tool_router(router = scanner_router, vis = "pub(crate)")]
impl McpHandler {
    #[tool(
        name = "run_scanner",
        description = "Run the IBKR market scanner using a named auto-scanner profile (configured under `auto_scanner.profiles` in settings). Returns the raw scanner rows (rank, contract, leg). Does NOT promote rows into the watchlist — that's only done by the auto-scanner background sweep. Use this for ad-hoc discovery: 'Show me top % gainers right now,' 'What's hitting volume breakouts?' Errors with 'unknown profile' if `profile_name` isn't in settings. Returns `{ items: [ScannerData, ...], count: N }`."
    )]
    pub async fn run_scanner(
        &self,
        Parameters(args): Parameters<RunScannerArgs>,
    ) -> Result<CallToolResult, McpError> {
        let name = args.profile_name.trim();
        if name.is_empty() {
            return map_tool_result::<(), &str>(Err("profile_name must not be empty"));
        }
        let cfg = self.auto_scanner.config().await;
        let Some(profile) = cfg.profiles.iter().find(|p| p.name == name) else {
            return map_tool_result::<(), String>(Err(format!(
                "unknown profile: {name}; configure it in settings under auto_scanner.profiles"
            )));
        };
        let subscription = subscription_for(profile);
        let result = self
            .market_scanner
            .scan(subscription)
            .await
            .map_err(|e| e.to_string());
        map_tool_result(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::settings::{AutoScannerConfig, ScanProfile};
    use crate::ibkr::mocks::MockIbkrClient;
    use crate::ibkr::types::{ContractDetails, ScannerData, SecurityType};
    use crate::mcp::tools::test_support::{handler_for_mock_ibkr, make_db};
    use std::sync::Arc;

    fn scan_row(symbol: &str, rank: i32) -> ScannerData {
        ScannerData {
            rank,
            contract: ContractDetails {
                symbol: symbol.to_string(),
                sec_type: SecurityType::Stock,
                exchange: "SMART".to_string(),
                primary_exchange: "NASDAQ".to_string(),
                currency: "USD".to_string(),
                local_symbol: symbol.to_string(),
                trading_class: symbol.to_string(),
                contract_id: 1,
                min_tick: 0.01,
                multiplier: "".to_string(),
                price_magnifier: 1,
            },
            leg: "".to_string(),
        }
    }

    fn profile_top_gainers() -> ScanProfile {
        ScanProfile {
            name: "top_gainers".to_string(),
            scan_code: "TOP_PERC_GAIN".to_string(),
            location_code: "STK.US.MAJOR".to_string(),
            above_price: Some(5.0),
            above_volume: Some(500_000),
            industry_filter: None,
            promote_top_k: 5,
            number_of_rows: 25,
        }
    }

    #[tokio::test]
    async fn run_scanner_uses_named_profile() {
        let (_tmp, db) = make_db();
        let mock = Arc::new(MockIbkrClient::new());
        // Program the canned scan results for the (scan_code,
        // industry) key the profile resolves to.
        mock.set_scan_results(
            "TOP_PERC_GAIN",
            None,
            vec![scan_row("NVDA", 1), scan_row("AMD", 2)],
        )
        .await;
        let handler = handler_for_mock_ibkr(db, mock).await;

        // Replace the default config with one that contains our test
        // profile. `set_config` is exposed on `AutoScannerService` for
        // exactly this kind of test seam.
        let cfg = AutoScannerConfig {
            enabled: false,
            interval_minutes: 30,
            daily_cap: 10,
            profiles: vec![profile_top_gainers()],
            industries: vec![],
        };
        handler.auto_scanner.set_config(cfg).await;

        let result = handler
            .run_scanner(Parameters(RunScannerArgs {
                profile_name: "top_gainers".to_string(),
            }))
            .await
            .expect("tool ok");
        assert_eq!(result.is_error, Some(false), "{:?}", result);
        let body = result
            .structured_content
            .as_ref()
            .expect("structured_content");
        assert_eq!(body["count"].as_u64().unwrap(), 2);
        let arr = body["items"].as_array().expect("items array");
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["contract"]["symbol"].as_str().unwrap(), "NVDA");
        assert_eq!(arr[0]["rank"].as_i64().unwrap(), 1);
        assert_eq!(arr[1]["contract"]["symbol"].as_str().unwrap(), "AMD");
    }

    #[tokio::test]
    async fn run_scanner_unknown_profile_returns_domain_error() {
        let (_tmp, db) = make_db();
        let mock = Arc::new(MockIbkrClient::new());
        let handler = handler_for_mock_ibkr(db, mock).await;
        // Default config from `handler_for_mock_ibkr` ships two
        // broad profiles ("Top % Gainers", "Hot by Volume") — both
        // with names that won't match "totally_made_up".
        let result = handler
            .run_scanner(Parameters(RunScannerArgs {
                profile_name: "totally_made_up".to_string(),
            }))
            .await
            .expect("rmcp Ok");
        assert_eq!(result.is_error, Some(true));
        let txt = result
            .content
            .first()
            .and_then(|c| c.as_text())
            .expect("text");
        assert!(txt.text.contains("unknown profile"), "got: {}", txt.text);
        assert!(txt.text.contains("totally_made_up"), "got: {}", txt.text);
    }

    #[tokio::test]
    async fn run_scanner_empty_profile_name_returns_domain_error() {
        let (_tmp, db) = make_db();
        let mock = Arc::new(MockIbkrClient::new());
        let handler = handler_for_mock_ibkr(db, mock).await;

        let result = handler
            .run_scanner(Parameters(RunScannerArgs {
                profile_name: "  ".to_string(),
            }))
            .await
            .expect("rmcp Ok");
        assert_eq!(result.is_error, Some(true));
        let txt = result
            .content
            .first()
            .and_then(|c| c.as_text())
            .expect("text");
        assert!(txt.text.contains("profile_name"), "got: {}", txt.text);
    }
}
