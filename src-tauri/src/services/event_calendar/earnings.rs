//! Earnings calendar — trait + composite implementation.
//!
//! `EarningsCalendar` is the seam every earnings-date lookup goes
//! through. The composite implementation layers:
//!   1. Manual override store (operator-curated, always wins)
//!   2. Cache store (refresh-weekly memo)
//!   3. Optional upstream fetcher (AV adapter — wired in P5 as a stub
//!      that returns `Ok(None)`; production wiring deferred per the
//!      AV-fundamentals retirement audit recorded in QUESTIONS.md).
//!
//! The trait is `async_trait` so AV-backed adapters can do real I/O.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use thiserror::Error;
use tracing::warn;

use crate::storage::error::StorageError;

use super::earnings_store::{EarningsCacheStore, EarningsOverridesStore};
use super::types::BlackoutConfidence;

/// What every earnings lookup returns: the next *upcoming* earnings
/// announcement date for `symbol`, or `None` when no source has data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EarningsEntry {
    pub symbol: String,
    pub date: NaiveDate,
    pub confidence: BlackoutConfidence,
    /// Provenance — "manual" / "cache" / "alpha_vantage". Persisted
    /// onto the gated setup so post-hoc audit can trace the decision.
    pub source: String,
}

#[derive(Debug, Error)]
pub enum EarningsError {
    #[error("storage: {0}")]
    Storage(#[from] StorageError),
    #[error("upstream: {0}")]
    Upstream(String),
}

pub type Result<T> = std::result::Result<T, EarningsError>;

#[async_trait]
pub trait EarningsCalendar: Send + Sync {
    /// Look up the next earnings date for `symbol` at instant `now`.
    /// Returns `Ok(None)` when no source has data — callers honor
    /// `skip_if_unknown` from the per-detector policy.
    async fn next_earnings_date(
        &self,
        symbol: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<EarningsEntry>>;
}

/// Optional upstream fetcher used by the composite when manual + cache
/// miss. Implementations refresh the cache as a side effect. The P5
/// production wiring is `NoOpUpstream`; an AV-backed impl can land
/// later behind the same trait.
#[async_trait]
pub trait UpstreamEarningsFetcher: Send + Sync {
    async fn fetch(&self, symbol: &str, now: DateTime<Utc>) -> Result<Option<EarningsEntry>>;
}

/// Default upstream stub. Returns `Ok(None)` for every symbol so the
/// composite's third tier is a graceful no-op until an AV-backed
/// impl is wired.
pub struct NoOpUpstream;

#[async_trait]
impl UpstreamEarningsFetcher for NoOpUpstream {
    async fn fetch(&self, _symbol: &str, _now: DateTime<Utc>) -> Result<Option<EarningsEntry>> {
        Ok(None)
    }
}

/// Composite calendar: manual > cache (fresh) > upstream.
pub struct CompositeEarningsCalendar {
    overrides: Arc<EarningsOverridesStore>,
    cache: Arc<EarningsCacheStore>,
    upstream: Arc<dyn UpstreamEarningsFetcher>,
}

impl CompositeEarningsCalendar {
    pub fn new(
        overrides: Arc<EarningsOverridesStore>,
        cache: Arc<EarningsCacheStore>,
        upstream: Arc<dyn UpstreamEarningsFetcher>,
    ) -> Self {
        Self {
            overrides,
            cache,
            upstream,
        }
    }

    /// Discard all cached rows — next lookup re-fetches from upstream
    /// (or returns `None` if upstream is the no-op stub). Wired to the
    /// `event_calendar_force_refresh` Tauri command.
    pub async fn force_refresh(&self) -> Result<()> {
        self.cache.clear_all().await?;
        Ok(())
    }
}

#[async_trait]
impl EarningsCalendar for CompositeEarningsCalendar {
    async fn next_earnings_date(
        &self,
        symbol: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<EarningsEntry>> {
        let key = symbol.trim().to_uppercase();
        if key.is_empty() {
            return Ok(None);
        }

        // 1. Manual override always wins.
        if let Some(row) = self.overrides.get(&key).await? {
            return Ok(Some(EarningsEntry {
                symbol: row.symbol,
                date: row.next_earnings_date,
                confidence: row.confidence,
                source: "manual".to_string(),
            }));
        }

        // 2. Cache hit — fresh wins outright; stale falls through to
        //    upstream and only serves if upstream errors / returns None.
        let cached_fresh = match self.cache.get(&key).await? {
            Some(row) if EarningsCacheStore::is_fresh(&row, now) => Some(row),
            _ => None,
        };
        if let Some(row) = cached_fresh {
            return Ok(Some(EarningsEntry {
                symbol: row.symbol,
                date: row.next_earnings_date,
                confidence: row.confidence,
                source: row.source,
            }));
        }

        // 3. Upstream fetch (writes the cache itself if it returns
        //    something usable). On error, fall back to stale cache.
        match self.upstream.fetch(&key, now).await {
            Ok(Some(entry)) => Ok(Some(entry)),
            Ok(None) | Err(_) => {
                if let Some(row) = self.cache.get(&key).await? {
                    warn!(
                        "earnings: upstream miss for {} — serving stale cache (fetched {} days ago)",
                        key,
                        ((now.timestamp() - row.fetched_at_unix) / 86_400).max(0)
                    );
                    return Ok(Some(EarningsEntry {
                        symbol: row.symbol,
                        date: row.next_earnings_date,
                        confidence: row.confidence,
                        source: row.source,
                    }));
                }
                Ok(None)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::storage::Db;
    use chrono::TimeZone;
    use tempfile::NamedTempFile;

    struct FixedUpstream {
        entry: Mutex<Option<EarningsEntry>>,
        cache: Arc<EarningsCacheStore>,
    }

    impl FixedUpstream {
        fn new(entry: Option<EarningsEntry>, cache: Arc<EarningsCacheStore>) -> Self {
            Self {
                entry: Mutex::new(entry),
                cache,
            }
        }
    }

    #[async_trait]
    impl UpstreamEarningsFetcher for FixedUpstream {
        async fn fetch(&self, symbol: &str, _now: DateTime<Utc>) -> Result<Option<EarningsEntry>> {
            let value = self.entry.lock().unwrap().clone();
            if let Some(entry) = &value {
                if entry.symbol == symbol {
                    self.cache
                        .upsert(symbol, entry.date, entry.confidence, &entry.source)
                        .await?;
                }
            }
            Ok(value)
        }
    }

    fn open_db() -> (NamedTempFile, Arc<Db>) {
        let tmp = NamedTempFile::new().unwrap();
        let db = Arc::new(Db::open(tmp.path()).unwrap());
        (tmp, db)
    }

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 5, 6, 14, 0, 0).unwrap()
    }

    #[tokio::test]
    async fn manual_override_wins_over_cache_and_upstream() {
        let (_tmp, db) = open_db();
        let overrides = Arc::new(EarningsOverridesStore::new(Arc::clone(&db)));
        let cache = Arc::new(EarningsCacheStore::new(Arc::clone(&db)));

        // Pre-populate cache with one date and override with another;
        // the override must win.
        cache
            .upsert(
                "AAPL",
                NaiveDate::from_ymd_opt(2026, 6, 1).unwrap(),
                BlackoutConfidence::Estimated,
                "alpha_vantage",
            )
            .await
            .unwrap();
        overrides
            .upsert(
                "AAPL",
                NaiveDate::from_ymd_opt(2026, 5, 15).unwrap(),
                BlackoutConfidence::Confirmed,
                "human",
                None,
            )
            .await
            .unwrap();

        let upstream: Arc<dyn UpstreamEarningsFetcher> = Arc::new(NoOpUpstream);
        let composite = CompositeEarningsCalendar::new(overrides, cache, upstream);

        let entry = composite
            .next_earnings_date("AAPL", now())
            .await
            .unwrap()
            .expect("manual override must surface");
        assert_eq!(entry.date, NaiveDate::from_ymd_opt(2026, 5, 15).unwrap());
        assert_eq!(entry.confidence, BlackoutConfidence::Confirmed);
        assert_eq!(entry.source, "manual");
    }

    #[tokio::test]
    async fn fresh_cache_short_circuits_upstream() {
        let (_tmp, db) = open_db();
        let overrides = Arc::new(EarningsOverridesStore::new(Arc::clone(&db)));
        let cache = Arc::new(EarningsCacheStore::new(Arc::clone(&db)));
        cache
            .upsert(
                "MSFT",
                NaiveDate::from_ymd_opt(2026, 7, 25).unwrap(),
                BlackoutConfidence::Estimated,
                "alpha_vantage",
            )
            .await
            .unwrap();

        // Upstream would return a different date — we should not see it.
        let upstream: Arc<dyn UpstreamEarningsFetcher> = Arc::new(FixedUpstream::new(
            Some(EarningsEntry {
                symbol: "MSFT".to_string(),
                date: NaiveDate::from_ymd_opt(2026, 9, 1).unwrap(),
                confidence: BlackoutConfidence::Estimated,
                source: "alpha_vantage".to_string(),
            }),
            Arc::clone(&cache),
        ));
        let composite = CompositeEarningsCalendar::new(overrides, cache, upstream);

        let entry = composite
            .next_earnings_date("MSFT", now())
            .await
            .unwrap()
            .expect("fresh cache must surface");
        assert_eq!(entry.date, NaiveDate::from_ymd_opt(2026, 7, 25).unwrap());
    }

    #[tokio::test]
    async fn upstream_populates_cache_on_miss() {
        let (_tmp, db) = open_db();
        let overrides = Arc::new(EarningsOverridesStore::new(Arc::clone(&db)));
        let cache = Arc::new(EarningsCacheStore::new(Arc::clone(&db)));
        let upstream: Arc<dyn UpstreamEarningsFetcher> = Arc::new(FixedUpstream::new(
            Some(EarningsEntry {
                symbol: "NVDA".to_string(),
                date: NaiveDate::from_ymd_opt(2026, 8, 28).unwrap(),
                confidence: BlackoutConfidence::Estimated,
                source: "alpha_vantage".to_string(),
            }),
            Arc::clone(&cache),
        ));
        let composite =
            CompositeEarningsCalendar::new(Arc::clone(&overrides), Arc::clone(&cache), upstream);

        let _ = composite.next_earnings_date("NVDA", now()).await.unwrap();
        let row = cache.get("NVDA").await.unwrap().expect("cache populated");
        assert_eq!(
            row.next_earnings_date,
            NaiveDate::from_ymd_opt(2026, 8, 28).unwrap()
        );
    }

    #[tokio::test]
    async fn no_source_returns_none() {
        let (_tmp, db) = open_db();
        let overrides = Arc::new(EarningsOverridesStore::new(Arc::clone(&db)));
        let cache = Arc::new(EarningsCacheStore::new(Arc::clone(&db)));
        let upstream: Arc<dyn UpstreamEarningsFetcher> = Arc::new(NoOpUpstream);
        let composite = CompositeEarningsCalendar::new(overrides, cache, upstream);

        let entry = composite.next_earnings_date("ZZZZ", now()).await.unwrap();
        assert!(entry.is_none());
    }
}
