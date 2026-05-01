//! Phase 3 — Social-sentiment ingestion.
//!
//! `SocialSentimentService` fans out to one or more
//! [`SentimentProvider`]s (Reddit / Stocktwits / Apewisdom in v1),
//! normalises every response into a [`SentimentSample`], and persists
//! the batch in a single transaction. The MCP `get_sentiment` tool and
//! the Tauri command read back through [`repo`] only — the service is
//! never on the read path.
//!
//! Test seam: providers and the wall-clock `fetched_at` are injected,
//! so unit tests construct a service with [`MockHttpFetcher`]-backed
//! providers and a fixed timestamp without touching the network.

pub mod apewisdom;
pub mod provider;
pub mod reddit;
pub mod repo;
pub mod stocktwits;
pub mod ticker_filter;
pub mod types;

use std::sync::Arc;

use chrono::Utc;
use tracing::{info, warn};

use crate::services::social_sentiment::provider::SentimentProvider;
use crate::services::social_sentiment::types::{
    SentimentSample, SentimentSource, SocialSentimentRow, SourceSummary,
};
use crate::storage::Db;

/// Outcome of one [`SocialSentimentService::fetch_and_persist`] run —
/// the scheduler logs it and the bare-bones tests assert on it.
#[derive(Debug, Clone)]
pub struct SentimentTickOutcome {
    pub fetched_at: i64,
    pub samples_persisted: usize,
    pub providers_failed: Vec<String>,
}

/// Trait object alias so the constructor reads cleanly.
pub type ArcProvider = Arc<dyn SentimentProvider>;

#[derive(Clone)]
pub struct SocialSentimentService {
    db: Arc<Db>,
    providers: Vec<ArcProvider>,
    clock: Arc<dyn ClockFn>,
}

/// Tiny clock seam — production wires `Utc::now().timestamp()`,
/// tests pin a fixed value. Lives here rather than reusing the
/// `LlmClock` because that one's typed for unix-seconds in a
/// budget-ledger-specific way and we don't want a one-way coupling.
pub trait ClockFn: Send + Sync {
    fn now_unix(&self) -> i64;
}

pub struct SystemClock;
impl ClockFn for SystemClock {
    fn now_unix(&self) -> i64 {
        Utc::now().timestamp()
    }
}

/// Test clock with an interior-mutable counter. Public for use from
/// the scheduler's tests; gated behind `cfg(test)` in the inner module.
#[cfg(test)]
pub struct FixedClock(pub std::sync::atomic::AtomicI64);
#[cfg(test)]
impl ClockFn for FixedClock {
    fn now_unix(&self) -> i64 {
        self.0.load(std::sync::atomic::Ordering::Relaxed)
    }
}

impl SocialSentimentService {
    pub fn new(db: Arc<Db>, providers: Vec<ArcProvider>) -> Self {
        Self {
            db,
            providers,
            clock: Arc::new(SystemClock),
        }
    }

    pub fn with_clock(mut self, clock: Arc<dyn ClockFn>) -> Self {
        self.clock = clock;
        self
    }

    /// Run every provider for `symbols` in parallel. Errors per
    /// provider are caught: each failure becomes one stale row per
    /// requested symbol so the agent can distinguish "tried, no
    /// signal" from "never asked." Returns the persisted count.
    pub async fn fetch_and_persist(
        &self,
        symbols: &[String],
    ) -> Result<SentimentTickOutcome, String> {
        let fetched_at = self.clock.now_unix();
        if symbols.is_empty() || self.providers.is_empty() {
            return Ok(SentimentTickOutcome {
                fetched_at,
                samples_persisted: 0,
                providers_failed: Vec::new(),
            });
        }

        let mut futures = Vec::with_capacity(self.providers.len());
        for provider in &self.providers {
            let p = Arc::clone(provider);
            let syms = symbols.to_vec();
            futures.push(tokio::spawn(async move {
                let id = p.id().to_string();
                let result = p.fetch(&syms).await;
                (id, syms, result)
            }));
        }

        let mut samples: Vec<SentimentSample> = Vec::new();
        let mut providers_failed: Vec<String> = Vec::new();
        for handle in futures {
            match handle.await {
                Ok((id, syms, Ok(mut rows))) => {
                    if rows.is_empty() {
                        // No upstream signal — persist a stale row per
                        // requested symbol for this provider.
                        for sym in &syms {
                            samples.push(stale(&id, sym));
                        }
                    } else {
                        // Backfill stale rows for requested symbols not
                        // covered by the provider's response.
                        let returned: std::collections::HashSet<String> =
                            rows.iter().map(|s| s.symbol.clone()).collect();
                        for sym in &syms {
                            if !returned.contains(&sym.to_ascii_uppercase()) {
                                samples.push(stale(&id, sym));
                            }
                        }
                        samples.append(&mut rows);
                    }
                }
                Ok((id, syms, Err(e))) => {
                    warn!("provider `{id}` failed: {e}");
                    providers_failed.push(id.clone());
                    for sym in &syms {
                        samples.push(stale(&id, sym));
                    }
                }
                Err(e) => {
                    warn!("provider task join failed: {e}");
                }
            }
        }

        let count = if samples.is_empty() {
            0
        } else {
            repo::insert_samples(Arc::clone(&self.db), samples, fetched_at)
                .await
                .map_err(|e| format!("persist samples: {e}"))?
        };

        info!(
            "social-sentiment tick: persisted {} rows, {} providers failed",
            count,
            providers_failed.len()
        );
        Ok(SentimentTickOutcome {
            fetched_at,
            samples_persisted: count,
            providers_failed,
        })
    }

    /// Build the agent-facing snapshot: latest row + samples-in-window
    /// per source. `since` is unix-seconds; pass `now - 24h` for the
    /// default 24h window.
    pub async fn snapshot(
        &self,
        symbol: &str,
        since: i64,
        sources: Option<Vec<String>>,
    ) -> Result<Vec<SourceSummary>, String> {
        let symbol_upper = symbol.to_uppercase();
        let rows = repo::rows_for_symbol_since(
            Arc::clone(&self.db),
            symbol_upper.clone(),
            since,
            sources,
        )
        .await
        .map_err(|e| format!("rows_for_symbol_since: {e}"))?;

        // Group by source preserving newest-first order.
        let mut by_source: std::collections::BTreeMap<String, Vec<SocialSentimentRow>> =
            std::collections::BTreeMap::new();
        for row in rows {
            by_source.entry(row.source.clone()).or_default().push(row);
        }
        let summaries: Vec<SourceSummary> = by_source
            .into_iter()
            .map(|(source, samples)| SourceSummary {
                latest: samples.first().cloned(),
                samples,
                source,
            })
            .collect();
        Ok(summaries)
    }
}

fn stale(source_id: &str, symbol: &str) -> SentimentSample {
    let source = SentimentSource::from_str(source_id).unwrap_or(SentimentSource::Apewisdom);
    SentimentSample {
        source,
        symbol: symbol.to_ascii_uppercase(),
        score: None,
        mentions_24h: None,
        label: None,
        rank: None,
        raw_payload: "{}".to_string(),
        is_stale: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::social_sentiment::apewisdom::ApewisdomProvider;
    use crate::services::social_sentiment::provider::MockHttpFetcher;
    use crate::services::social_sentiment::reddit::RedditWsbProvider;
    use crate::services::social_sentiment::stocktwits::StocktwitsProvider;
    use std::sync::atomic::AtomicI64;
    use tempfile::NamedTempFile;

    fn make_db() -> (NamedTempFile, Arc<Db>) {
        let tmp = NamedTempFile::new().expect("tempfile");
        let db = Db::open(tmp.path()).expect("open db");
        (tmp, Arc::new(db))
    }

    #[tokio::test]
    async fn fetch_and_persist_writes_rows_and_fills_stale_for_missing_symbols() {
        let (_tmp, db) = make_db();
        let now = 1_700_000_321_i64;
        let clock: Arc<dyn ClockFn> = Arc::new(FixedClock(AtomicI64::new(now)));

        let http_ape = Arc::new(MockHttpFetcher::new());
        http_ape.respond_with(
            "https://test.local/ape",
            r#"{"results":[{"ticker":"TSLA","rank":1,"mentions":99,
                            "sentiment":"Bullish","sentiment_score":75.0}]}"#,
        );
        let ape: ArcProvider = Arc::new(
            ApewisdomProvider::new(http_ape).with_url("https://test.local/ape"),
        );

        let http_st = Arc::new(MockHttpFetcher::new());
        // Stocktwits has nothing for AMD and errors for TSLA -> stale rows for both.
        let st: ArcProvider = Arc::new(
            StocktwitsProvider::new(http_st).with_base_url("https://test.local/twits"),
        );

        let http_red = Arc::new(MockHttpFetcher::new());
        http_red.respond_with(
            "https://test.local/wsb.json",
            r#"{"data":{"children":[
                {"data":{"title":"TSLA pumping $AMD too","selftext":""}}
            ]}}"#,
        );
        let red: ArcProvider = Arc::new(
            RedditWsbProvider::new(http_red).with_url("https://test.local/wsb.json"),
        );

        let svc = SocialSentimentService::new(db, vec![ape, st, red]).with_clock(clock);
        let symbols = vec!["TSLA".to_string(), "AMD".to_string()];
        let outcome = svc.fetch_and_persist(&symbols).await.expect("ok");

        // 3 providers x 2 symbols = 6 rows expected (apewisdom returns
        // TSLA only -> AMD stale; stocktwits errors -> 2 stale; reddit
        // returns counts for both).
        assert_eq!(outcome.samples_persisted, 6, "two rows per provider");
        assert_eq!(outcome.fetched_at, now);
        // Stocktwits is "failed" only when it returns Err — here it
        // produces stale rows per symbol via the in-provider catch, so
        // providers_failed stays empty. Re-verify via repo:
        let snap = svc
            .snapshot("TSLA", now - 86_400, None)
            .await
            .expect("snapshot");
        assert!(!snap.is_empty());
        assert!(snap.iter().any(|s| s.source == "apewisdom" && s.latest.as_ref().unwrap().score == Some(0.75)));
        assert!(snap.iter().any(|s| s.source == "reddit_wsb"));
    }

    #[tokio::test]
    async fn empty_symbols_short_circuits_without_db_write() {
        let (_tmp, db) = make_db();
        let svc = SocialSentimentService::new(db, vec![]);
        let outcome = svc.fetch_and_persist(&[]).await.expect("ok");
        assert_eq!(outcome.samples_persisted, 0);
    }
}
