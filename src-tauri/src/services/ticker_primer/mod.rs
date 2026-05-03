//! Ticker-intake Phase 1 — `TickerPrimerService`.
//!
//! Spawned (fire-and-forget) by both `add_ticker` callers (the MCP tool and
//! the `tracker_add` Tauri command) immediately after the watchlist row is
//! inserted. Chains three existing services so the user sees populated
//! projection + news panels seconds after add:
//!
//! ```text
//! FundamentalsProvider::fetch
//!   → ProjectionService::generate_projection_results (sync, no IO)
//!   → CacheService::write("{SYM}_projection", &results)
//!   → NewsProvider::fetch (transparently warms `news_cache` and runs
//!     `NewsInterpreter` per the existing `IbkrNewsProvider` wiring).
//! ```
//!
//! No new LLM call sites; the LLM spend that lands during prime is the
//! existing news-interpreter pass, already gated by `LlmService` budget.
//! Idempotent on `tracked_tickers.last_primed_at < 24h` so a UI re-add
//! storm cannot burn provider budget. `archive_ticker` clears
//! `last_primed_at` so a re-prime fires after unarchive.

use std::sync::Arc;

use chrono::{Duration as ChronoDuration, Utc};
use tracing::{debug, info, warn};

use crate::events::{AppEvent, EventEmitter};
use crate::ibkr::types::tracker::{TickerPrimingOutcome, TickerPrimingStepStatus};
use crate::ibkr::types::ProjectionAssumptions;
use crate::services::cache_service::CacheService;
use crate::services::fundamentals_provider::{FundamentalsError, FundamentalsProvider};
use crate::services::news_provider::NewsProvider;
use crate::services::projection_service::ProjectionService;
use crate::services::tracker_service::TrackerService;

#[cfg(test)]
mod tests;

/// Idempotency window: re-priming a symbol within this many hours of a
/// successful prime is a no-op so a UI re-add storm cannot burn provider
/// budget. Mirrors the master plan's "Prime idempotency window: 24h"
/// default; `archive_ticker` clears the watermark so unarchive picks up
/// a fresh prime regardless of how recent the previous one was.
const PRIME_IDEMPOTENCY_HOURS: i64 = 24;

/// Lookback for the prime-time news fetch. The EOD `TrackerRunner`
/// retunes its own lookback per cadence; this path's job is "warm the
/// news panel right after add", and 24h matches the master plan's
/// default while keeping the `NewsInterpreter` prompt size bounded.
const PRIME_NEWS_LOOKBACK_HOURS: u32 = 24;

/// Cache-key shape for the projection JSON. The 7-day TTL is owned by
/// `CacheService::new` (master plan default). Keeping the key namespaced
/// with `_projection` keeps it disjoint from the AV `{SYM}_overview` /
/// `{SYM}_income_statement` / `{SYM}_earnings` keys that share the same
/// "cache/alphavantage" directory in production wiring.
fn projection_cache_key(symbol: &str) -> String {
    format!("{}_projection", symbol)
}

/// Compose post-add fundamentals + projection + news priming. Holds
/// `Arc` clones of every dependency so the spawned future is
/// `'static + Send`.
pub struct TickerPrimerService {
    tracker: Arc<TrackerService>,
    fundamentals: Arc<dyn FundamentalsProvider>,
    news: Arc<dyn NewsProvider>,
    cache: Arc<CacheService>,
    emitter: Arc<EventEmitter>,
    assumptions: ProjectionAssumptions,
}

impl TickerPrimerService {
    pub fn new(
        tracker: Arc<TrackerService>,
        fundamentals: Arc<dyn FundamentalsProvider>,
        news: Arc<dyn NewsProvider>,
        cache: Arc<CacheService>,
        emitter: Arc<EventEmitter>,
    ) -> Self {
        Self {
            tracker,
            fundamentals,
            news,
            cache,
            emitter,
            assumptions: ProjectionAssumptions::default(),
        }
    }

    /// Run the post-add chain for `symbol`. Idempotent on
    /// `last_primed_at < 24h` — the fast path emits a `Skipped` outcome
    /// without touching any provider. Returns the outcome so callers
    /// (today: spawn-callers that ignore it) can assert in tests; the
    /// outcome is also broadcast via `AppEvent::TickerPrimingDone`.
    ///
    /// Errors are logged and folded into the per-step status rather
    /// than surfaced upstream — every production caller is a
    /// fire-and-forget spawn.
    pub async fn prime(&self, symbol: &str) -> TickerPrimingOutcome {
        let symbol_norm = symbol.trim().to_uppercase();

        if self.is_recently_primed(&symbol_norm).await {
            debug!("ticker_primer skip {symbol_norm} — primed within {PRIME_IDEMPOTENCY_HOURS}h");
            let outcome = TickerPrimingOutcome {
                fundamentals: TickerPrimingStepStatus::Skipped,
                projection: TickerPrimingStepStatus::Skipped,
                news: TickerPrimingStepStatus::Skipped,
                primed_at: Utc::now(),
            };
            self.emit(&symbol_norm, outcome.clone()).await;
            return outcome;
        }

        info!("ticker_primer start {symbol_norm}");

        // Step 1: fundamentals. NotFound → NoData (upstream healthy, just
        // no rows for this symbol); other errors → Err(message). Either
        // way we still attempt news so the panel warms.
        let (fundamentals_status, fundamental_data) =
            match self.fundamentals.fetch(&symbol_norm).await {
                Ok(data) => (TickerPrimingStepStatus::Ok, Some(data)),
                Err(FundamentalsError::NotFound(_)) => (TickerPrimingStepStatus::NoData, None),
                Err(e) => {
                    warn!("ticker_primer fundamentals failed for {symbol_norm}: {e}");
                    (TickerPrimingStepStatus::Err(e.to_string()), None)
                }
            };

        // Step 2: projection (sync; cached as `{SYM}_projection`). Skipped
        // when fundamentals lack a baseline year (the projection function
        // returns "No historical data available" — surface as NoData,
        // not Err, since this is a known empty-data shape).
        let projection_status = match fundamental_data.as_ref() {
            None => TickerPrimingStepStatus::NoData,
            Some(fd) => {
                match ProjectionService::generate_projection_results(fd, &self.assumptions) {
                    Ok(results) => {
                        match self
                            .cache
                            .write(&projection_cache_key(&symbol_norm), &results)
                        {
                            Ok(_) => TickerPrimingStepStatus::Ok,
                            Err(e) => {
                                warn!(
                                    "ticker_primer projection cache write failed for {symbol_norm}: {e}"
                                );
                                TickerPrimingStepStatus::Err(format!("cache write: {e}"))
                            }
                        }
                    }
                    Err(e) => {
                        info!("ticker_primer projection skipped for {symbol_norm}: {e}");
                        TickerPrimingStepStatus::NoData
                    }
                }
            }
        };

        // Step 3: news. Always attempted — the `IbkrNewsProvider` wiring
        // populates `news_cache` and runs the `NewsInterpreter` verdict
        // pass on a successful, non-empty fetch. No new LLM call site.
        let news_status = match self
            .news
            .fetch(&symbol_norm, PRIME_NEWS_LOOKBACK_HOURS)
            .await
        {
            Ok(items) if items.is_empty() => TickerPrimingStepStatus::NoData,
            Ok(_) => TickerPrimingStepStatus::Ok,
            Err(e) => {
                warn!("ticker_primer news failed for {symbol_norm}: {e}");
                TickerPrimingStepStatus::Err(e.to_string())
            }
        };

        // Stamp the watermark only when the fundamentals call itself
        // completed (success OR explicit "no data"). A hard fundamentals
        // failure leaves `last_primed_at` NULL so the next add gets a
        // real attempt — matches the plan's "What counts as 'primed'?"
        // decision.
        if !matches!(fundamentals_status, TickerPrimingStepStatus::Err(_)) {
            if let Err(e) = self.tracker.mark_primed(&symbol_norm).await {
                warn!("ticker_primer mark_primed failed for {symbol_norm}: {e}");
            }
        }

        let outcome = TickerPrimingOutcome {
            fundamentals: fundamentals_status,
            projection: projection_status,
            news: news_status,
            primed_at: Utc::now(),
        };
        info!("ticker_primer done {symbol_norm} outcome={:?}", outcome);
        self.emit(&symbol_norm, outcome.clone()).await;
        outcome
    }

    async fn is_recently_primed(&self, symbol: &str) -> bool {
        match self.tracker.get(symbol).await {
            Ok(Some(row)) => match row.last_primed_at {
                Some(last) => {
                    Utc::now().signed_duration_since(last)
                        < ChronoDuration::hours(PRIME_IDEMPOTENCY_HOURS)
                }
                None => false,
            },
            Ok(None) => false,
            Err(e) => {
                warn!("ticker_primer is_recently_primed lookup failed for {symbol}: {e}");
                false
            }
        }
    }

    async fn emit(&self, symbol: &str, outcome: TickerPrimingOutcome) {
        if let Err(e) = self
            .emitter
            .emit(AppEvent::TickerPrimingDone {
                symbol: symbol.to_string(),
                outcome,
            })
            .await
        {
            // Tests run without an app handle attached and rely on the
            // capture buffer. Production sinks via Tauri. Either way the
            // primer cannot recover, so log and move on.
            debug!("ticker_primer emit failed for {symbol}: {e}");
        }
    }
}
