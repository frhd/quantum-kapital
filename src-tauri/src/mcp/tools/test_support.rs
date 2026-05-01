//! Shared test scaffolding for the per-tool unit tests under `mcp/tools`.
//!
//! Lifted out of `handler.rs::tests` once Step 5 split tools into one file
//! per tool — every `tools/*.rs` test now needs the same `Db`-backed
//! `McpHandler` constructor plus a deterministic `LlmClock`.
//!
//! Also re-exports `test_handler_with_seeded_spend`, the `#[doc(hidden)]`
//! constructor consumed by `tests/mcp_tool_call.rs` (the cross-crate
//! integration test). It lives here so the integration test can reach it
//! without the rest of the test scaffolding being public; it intentionally
//! is **not** gated on `#[cfg(test)]` because integration tests compile the
//! library as a regular dependency when targeting `--release` profiles.

#![allow(dead_code)] // each helper is consumed by a subset of the per-tool tests.

use std::path::Path;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

#[cfg(test)]
use tempfile::NamedTempFile;

use crate::mcp::handler::McpHandler;
use crate::services::financial_data_service::FinancialDataService;
use crate::services::llm_service::{LlmClock, LlmService};
use crate::services::tracker_service::TrackerService;
use crate::storage::Db;

/// Deterministic [`LlmClock`] fixed at construction time. Lets budget /
/// spend tests pin "today" without leaking real wall-clock behaviour.
pub struct FixedClock(pub AtomicI64);

impl LlmClock for FixedClock {
    fn now_unix(&self) -> i64 {
        self.0.load(Ordering::Relaxed)
    }
}

/// Open a fresh on-disk SQLite Db for a single test. The `NamedTempFile`
/// is returned alongside so the caller keeps it alive for the test's
/// duration — dropping it deletes the underlying file.
#[cfg(test)]
pub fn make_db() -> (NamedTempFile, Arc<Db>) {
    let tmp = NamedTempFile::new().expect("tempfile");
    let db = Db::open(tmp.path()).expect("open db");
    (tmp, Arc::new(db))
}

/// Build an `McpHandler` wired to fresh services on top of the supplied
/// `Db`. Used by the data-tier tools (watchlist / setups / alerts / news)
/// which don't care about the LLM budget; the `LlmService` is constructed
/// with a no-op API key and a generous budget.
#[cfg(test)]
pub fn handler_for_db(db: Arc<Db>) -> McpHandler {
    let llm = Arc::new(LlmService::new(
        "test-key".to_string(),
        Arc::clone(&db),
        100.0,
    ));
    let tracker = Arc::new(TrackerService::new(Arc::clone(&db)));
    // Empty API key + empty base URL — every news fetch falls through to
    // the cache-only path. Tests that exercise the AV-fallback branch
    // override this by calling `FinancialDataService::new(...)` themselves.
    let financial = Arc::new(FinancialDataService::new(String::new()).with_db(Arc::clone(&db)));
    McpHandler::new(llm, tracker, db, financial)
}

/// Construct an `McpHandler` against a fresh on-disk DB at `db_path` with
/// a single seeded `llm_calls` row representing today's spend. The clock
/// is fixed at `2023-11-14 22:13:20 UTC`.
///
/// Lives here so the cross-crate integration test in
/// `tests/mcp_tool_call.rs` can construct a realistic handler without
/// pulling private internals of `LlmService` / `Db` / the test clock.
#[doc(hidden)]
pub async fn test_handler_with_seeded_spend(
    db_path: &Path,
    spent_today_usd: f64,
    daily_budget_usd: f64,
) -> std::io::Result<McpHandler> {
    let db =
        Arc::new(Db::open(db_path).map_err(|e| std::io::Error::other(format!("open db: {e}")))?);

    // 2023-11-14 22:13:20 UTC — well after that day's UTC midnight.
    let now: i64 = 1_700_000_000;
    let day_start: i64 = (now / 86_400) * 86_400;

    db.with_conn(move |conn| {
        conn.execute(
            "INSERT INTO llm_calls (kind, model, input_tokens, output_tokens, \
             cache_read_tokens, cost_usd, called_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                "thesis",
                "claude-sonnet-4-6",
                0i64,
                0i64,
                0i64,
                spent_today_usd,
                day_start
            ],
        )?;
        Ok(())
    })
    .await
    .map_err(|e| std::io::Error::other(format!("seed llm_calls: {e}")))?;

    let clock: Arc<dyn LlmClock> = Arc::new(FixedClock(AtomicI64::new(now)));
    let llm = Arc::new(
        LlmService::new("test-key".to_string(), Arc::clone(&db), daily_budget_usd)
            .with_clock(clock),
    );
    let tracker = Arc::new(TrackerService::new(Arc::clone(&db)));
    let financial = Arc::new(FinancialDataService::new(String::new()).with_db(Arc::clone(&db)));
    Ok(McpHandler::new(llm, tracker, db, financial))
}
