//! Phase 8 cutover parity check — runs the IBKR and AV news providers
//! side-by-side over a 5-symbol mix and prints a coverage report. The
//! plan (`loop/plan/phase-8-av-deletion.md`) wants this to confirm
//! IBKR coverage is materially close to AV's before the deletion
//! commit lands.
//!
//! `#[ignore]`-gated so CI does not need TWS or an AV API key. Run
//! manually:
//!
//! ```sh
//! cd src-tauri
//! cargo test --test news_provider_parity -- --ignored --nocapture
//! # Optional overrides:
//! QK_PARITY_HOST=127.0.0.1 QK_PARITY_PORT=7497 QK_PARITY_CLIENT_ID=902 \
//!     cargo test --test news_provider_parity -- --ignored --nocapture
//! ```
//!
//! Defaults: host `127.0.0.1`, port `4004` (paper Gateway, matching
//! the Phase 6 capture), client id `902` (above the 998 spike id, so
//! TWS does not reject as duplicate).
//!
//! Asserts only the *shape* of items returned (per the plan: "diff-
//! asserts the field set, excluding sentiment scores"). Per-item
//! content equality is impossible — IBKR and AV index different
//! providers — and per-article sentiment is intentionally lost on
//! the IBKR side per the Phase 6 sentiment-loss audit.

use std::env;
use std::sync::Arc;
use std::time::Duration;

use quantum_kapital_lib::news_parity_support::{
    AlphaVantageNewsProvider, ConnectionConfig, FinancialDataService, IbkrClient, IbkrNewsClient,
    IbkrNewsProvider, IbkrNewsRateLimiter, NewsItem, NewsProvider,
};

/// Mix chosen for the cutover smoke check: large-cap tech, large-cap
/// chip, mid-large diversified, ADR, small-cap volatile. Matches the
/// "Open risks → News coverage gap" item in the master plan.
const PARITY_SYMBOLS: &[&str] = &["AAPL", "AMD", "DIS", "TSM", "RIVN"];
const LOOKBACK_HOURS: u32 = 24;

/// Per-symbol pacing pause between IBKR fetches. The Phase 6 spike
/// captured at 2s; we mirror that here so the run does not trip
/// IBKR's per-minute pacing on a 5-symbol pass.
const IBKR_PACE: Duration = Duration::from_secs(2);

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires live TWS/Gateway and Alpha Vantage API key — run manually with --ignored"]
async fn ibkr_and_av_news_parity_over_five_symbols() {
    let ibkr_provider = build_ibkr_provider().await;
    let av_provider = build_av_provider();

    let mut report = ParityReport::default();
    for symbol in PARITY_SYMBOLS {
        let ibkr_items = match ibkr_provider.fetch(symbol, LOOKBACK_HOURS).await {
            Ok(items) => items,
            Err(e) => {
                eprintln!("IBKR fetch failed for {symbol}: {e}");
                report
                    .ibkr_failures
                    .push((symbol.to_string(), e.to_string()));
                Vec::new()
            }
        };
        let av_items = match av_provider.fetch(symbol, LOOKBACK_HOURS).await {
            Ok(items) => items,
            Err(e) => {
                eprintln!("AV fetch failed for {symbol}: {e}");
                report.av_failures.push((symbol.to_string(), e.to_string()));
                Vec::new()
            }
        };
        assert_well_shaped(symbol, "IBKR", &ibkr_items);
        assert_well_shaped(symbol, "AV", &av_items);
        report.rows.push(SymbolRow {
            symbol: symbol.to_string(),
            ibkr_count: ibkr_items.len(),
            av_count: av_items.len(),
        });
        tokio::time::sleep(IBKR_PACE).await;
    }

    report.print();

    // Soft assertion: IBKR returned at least one item for at least
    // three of the five symbols. Lower than that on a 24h window for
    // this mix is the threshold the plan wants escalated to
    // QUESTIONS.md before the deletion commit.
    let symbols_with_ibkr_items = report.rows.iter().filter(|r| r.ibkr_count > 0).count();
    assert!(
        symbols_with_ibkr_items >= 3,
        "IBKR coverage too thin: only {symbols_with_ibkr_items} of {} symbols returned headlines. \
         Either the subscribed provider mix is too narrow or the lookback window is empty. \
         Log the symbols + window in loop/plan/QUESTIONS.md before flipping Phase 8 to done.",
        PARITY_SYMBOLS.len(),
    );
}

async fn build_ibkr_provider() -> Arc<dyn NewsProvider> {
    let host = env::var("QK_PARITY_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let port: u16 = env::var("QK_PARITY_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(4004);
    let client_id: i32 = env::var("QK_PARITY_CLIENT_ID")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(902);

    let config = ConnectionConfig {
        host,
        port,
        client_id,
        ..Default::default()
    };
    let client = Arc::new(IbkrClient::new(config));
    client
        .connect()
        .await
        .expect("IBKR connect — is TWS/Gateway running with API access enabled?");

    let news_client: Arc<dyn IbkrNewsClient> = Arc::clone(&client) as Arc<dyn IbkrNewsClient>;
    let rate_limiter = Arc::new(IbkrNewsRateLimiter::new(30));
    Arc::new(IbkrNewsProvider::new(news_client, rate_limiter))
}

fn build_av_provider() -> Arc<dyn NewsProvider> {
    let api_key =
        env::var("ALPHA_VANTAGE_API_KEY").expect("ALPHA_VANTAGE_API_KEY env var required");
    let api_key_present = !api_key.trim().is_empty();
    let svc = Arc::new(FinancialDataService::new(api_key));
    Arc::new(AlphaVantageNewsProvider::new(svc, api_key_present))
}

/// Per-item shape check. Sentiment fields are intentionally excluded
/// — the Phase 6 sentiment-loss audit confirmed downstream consumers
/// tolerate IBKR's `None` sentiment.
fn assert_well_shaped(symbol: &str, label: &str, items: &[NewsItem]) {
    for (idx, item) in items.iter().enumerate() {
        assert!(
            !item.title.trim().is_empty(),
            "{label} {symbol}[{idx}] has empty title"
        );
        assert!(
            !item.source.trim().is_empty(),
            "{label} {symbol}[{idx}] has empty source"
        );
        // time_published is a non-Option DateTime — checking the year
        // is in the modern range catches obvious epoch-zero parser
        // regressions.
        let year = item.time_published.format("%Y").to_string();
        assert!(
            year.starts_with("20"),
            "{label} {symbol}[{idx}] time_published year looks bogus: {}",
            item.time_published
        );
    }
}

#[derive(Default)]
struct ParityReport {
    rows: Vec<SymbolRow>,
    ibkr_failures: Vec<(String, String)>,
    av_failures: Vec<(String, String)>,
}

struct SymbolRow {
    symbol: String,
    ibkr_count: usize,
    av_count: usize,
}

impl ParityReport {
    fn print(&self) {
        eprintln!(
            "\n=== news provider parity ({}h lookback) ===",
            LOOKBACK_HOURS
        );
        eprintln!(
            "{:<8}  {:>10}  {:>10}  {:>10}",
            "symbol", "ibkr", "av", "ratio"
        );
        for row in &self.rows {
            let ratio = if row.av_count == 0 {
                "—".to_string()
            } else {
                format!("{:.2}", row.ibkr_count as f64 / row.av_count as f64)
            };
            eprintln!(
                "{:<8}  {:>10}  {:>10}  {:>10}",
                row.symbol, row.ibkr_count, row.av_count, ratio
            );
        }
        if !self.ibkr_failures.is_empty() {
            eprintln!("\nIBKR failures:");
            for (s, e) in &self.ibkr_failures {
                eprintln!("  {s}: {e}");
            }
        }
        if !self.av_failures.is_empty() {
            eprintln!("\nAV failures:");
            for (s, e) in &self.av_failures {
                eprintln!("  {s}: {e}");
            }
        }
        eprintln!();
    }
}
