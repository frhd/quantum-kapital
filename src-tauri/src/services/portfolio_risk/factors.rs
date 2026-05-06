//! Phase 8 — coarse factor bucketing. Three factors only: momentum,
//! value, size. Bucketed coarsely (high / mid / low / unknown). No
//! factor model fitting; the goal is a heatmap that surfaces
//! "you're 4 deep in high-momentum names" before adding a 5th.
//!
//! Factor membership is computed lazily and cached per symbol with
//! a 7-day TTL (master gotcha "Don't compute factors live for every
//! snapshot"). Cache is in-memory; a restart re-computes.
//!
//! This module is intentionally light: each factor returns its
//! bucket as a `&'static str` so the snapshot exposure_json
//! payload stays compact and serializable.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::RwLock;

const CACHE_TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// One symbol's factor membership. Each field is a coarse bucket
/// label. `"unknown"` is the safe default when inputs are missing
/// (no historical bars, no fundamentals).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FactorMembership {
    pub momentum: &'static str,
    pub value: &'static str,
    pub size: &'static str,
}

impl FactorMembership {
    pub fn unknown() -> Self {
        Self {
            momentum: "unknown",
            value: "unknown",
            size: "unknown",
        }
    }
}

/// Inputs available for factor computation. The caller (the
/// snapshot path) collects these from the existing services
/// (`Position`, possibly fundamentals); fields are optional so a
/// missing data point degrades gracefully to `"unknown"` rather
/// than crashing the snapshot.
#[derive(Debug, Clone, Default)]
pub struct FactorInputs {
    /// Trailing 12-month return percentile in `[0.0, 1.0]`. Master
    /// committed "12-1m return percentile". Today: pulled from
    /// daily-bar history when available (the snapshot path can
    /// thread this in once the service is wired into the runner;
    /// pre-wire callers can pass `None`).
    pub return_12m_percentile: Option<f64>,
    /// Trailing P/E ratio percentile. Lower percentile → cheaper.
    pub pe_percentile: Option<f64>,
    /// Market cap in dollars (not millions). The size bucket cuts
    /// at $2B (small) / $10B (mid) / $200B (mega).
    pub market_cap_usd: Option<f64>,
}

/// Service-level cache. Threaded through `PortfolioRiskService`
/// and `ConcentrationGate` so multiple snapshots in a 7-day window
/// share factor lookups.
pub struct FactorBuckets {
    cache: RwLock<HashMap<String, (Instant, FactorMembership)>>,
}

impl FactorBuckets {
    pub fn new() -> Self {
        Self {
            cache: RwLock::new(HashMap::new()),
        }
    }

    pub fn arc() -> Arc<Self> {
        Arc::new(Self::new())
    }

    /// Look up a symbol's factor membership. Returns the cached
    /// value when fresh; otherwise computes from `inputs`, caches,
    /// and returns. `inputs` may be `None` for callers that don't
    /// have the data on hand — those reads stay `unknown` until a
    /// caller does provide inputs.
    pub async fn lookup_or_compute(
        &self,
        symbol: &str,
        inputs: Option<&FactorInputs>,
    ) -> FactorMembership {
        let upper = symbol.to_uppercase();
        if let Some((at, m)) = self.cache.read().await.get(&upper) {
            if at.elapsed() < CACHE_TTL {
                return *m;
            }
        }
        let computed = match inputs {
            Some(i) => Self::compute_from(i),
            None => FactorMembership::unknown(),
        };
        self.cache
            .write()
            .await
            .insert(upper, (Instant::now(), computed));
        computed
    }

    /// Pure compute — exposed for tests + for the synchronous
    /// snapshot path that already holds the inputs.
    pub fn compute_from(inputs: &FactorInputs) -> FactorMembership {
        FactorMembership {
            momentum: bucket_percentile(inputs.return_12m_percentile, "momentum"),
            value: bucket_percentile_inverted(inputs.pe_percentile, "value"),
            size: bucket_market_cap(inputs.market_cap_usd),
        }
    }

    /// Force-refresh a symbol's cache entry. Test seam.
    pub async fn invalidate(&self, symbol: &str) {
        self.cache.write().await.remove(&symbol.to_uppercase());
    }
}

impl Default for FactorBuckets {
    fn default() -> Self {
        Self::new()
    }
}

/// Bucket a percentile into `{prefix}_high` / `{prefix}_mid` /
/// `{prefix}_low`. Cuts at 0.66 / 0.33; NaN / out-of-range falls
/// to `unknown`.
fn bucket_percentile(p: Option<f64>, prefix: &'static str) -> &'static str {
    match p {
        Some(v) if v.is_finite() && (0.0..=1.0).contains(&v) => {
            if v >= 0.66 {
                bucket_label(prefix, "high")
            } else if v >= 0.33 {
                bucket_label(prefix, "mid")
            } else {
                bucket_label(prefix, "low")
            }
        }
        _ => "unknown",
    }
}

/// Same as `bucket_percentile` but the high/low labels are flipped:
/// a low P/E percentile means "cheap" → `value_high` (we have *more*
/// of the value factor). Keeps the semantic so the heatmap reads
/// "high momentum + high value" the way the trader expects.
fn bucket_percentile_inverted(p: Option<f64>, prefix: &'static str) -> &'static str {
    match p {
        Some(v) if v.is_finite() && (0.0..=1.0).contains(&v) => {
            if v <= 0.33 {
                bucket_label(prefix, "high")
            } else if v <= 0.66 {
                bucket_label(prefix, "mid")
            } else {
                bucket_label(prefix, "low")
            }
        }
        _ => "unknown",
    }
}

fn bucket_market_cap(cap: Option<f64>) -> &'static str {
    match cap {
        Some(v) if v.is_finite() && v > 0.0 => {
            if v >= 200_000_000_000.0 {
                "size_mega"
            } else if v >= 10_000_000_000.0 {
                "size_large"
            } else if v >= 2_000_000_000.0 {
                "size_mid"
            } else {
                "size_small"
            }
        }
        _ => "unknown",
    }
}

/// Composite the two-part label from compile-time constants. Match
/// arm so we return a `&'static str` (Rust's lifetimes don't let us
/// `format!()` a `&'static str`).
fn bucket_label(prefix: &'static str, level: &'static str) -> &'static str {
    match (prefix, level) {
        ("momentum", "high") => "momentum_high",
        ("momentum", "mid") => "momentum_mid",
        ("momentum", "low") => "momentum_low",
        ("value", "high") => "value_high",
        ("value", "mid") => "value_mid",
        ("value", "low") => "value_low",
        _ => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn momentum_bucket_thresholds() {
        let inputs = FactorInputs {
            return_12m_percentile: Some(0.9),
            pe_percentile: None,
            market_cap_usd: None,
        };
        let m = FactorBuckets::compute_from(&inputs);
        assert_eq!(m.momentum, "momentum_high");

        let mid = FactorBuckets::compute_from(&FactorInputs {
            return_12m_percentile: Some(0.5),
            ..Default::default()
        });
        assert_eq!(mid.momentum, "momentum_mid");

        let low = FactorBuckets::compute_from(&FactorInputs {
            return_12m_percentile: Some(0.1),
            ..Default::default()
        });
        assert_eq!(low.momentum, "momentum_low");
    }

    #[test]
    fn value_bucket_inverts_pe_percentile() {
        let cheap = FactorBuckets::compute_from(&FactorInputs {
            pe_percentile: Some(0.1), // cheap = high value
            ..Default::default()
        });
        assert_eq!(cheap.value, "value_high");

        let expensive = FactorBuckets::compute_from(&FactorInputs {
            pe_percentile: Some(0.9),
            ..Default::default()
        });
        assert_eq!(expensive.value, "value_low");
    }

    #[test]
    fn size_bucket_cap_thresholds() {
        let mega = FactorBuckets::compute_from(&FactorInputs {
            market_cap_usd: Some(500e9),
            ..Default::default()
        });
        assert_eq!(mega.size, "size_mega");

        let mid = FactorBuckets::compute_from(&FactorInputs {
            market_cap_usd: Some(5e9),
            ..Default::default()
        });
        assert_eq!(mid.size, "size_mid");

        let small = FactorBuckets::compute_from(&FactorInputs {
            market_cap_usd: Some(500e6),
            ..Default::default()
        });
        assert_eq!(small.size, "size_small");
    }

    #[test]
    fn nan_and_oob_inputs_collapse_to_unknown() {
        let m = FactorBuckets::compute_from(&FactorInputs {
            return_12m_percentile: Some(f64::NAN),
            pe_percentile: Some(1.5),
            market_cap_usd: Some(-1.0),
        });
        assert_eq!(m.momentum, "unknown");
        assert_eq!(m.value, "unknown");
        assert_eq!(m.size, "unknown");
    }

    #[tokio::test]
    async fn cache_round_trips() {
        let cache = FactorBuckets::new();
        let inputs = FactorInputs {
            return_12m_percentile: Some(0.8),
            pe_percentile: Some(0.2),
            market_cap_usd: Some(50e9),
            ..Default::default()
        };
        let m1 = cache.lookup_or_compute("AAPL", Some(&inputs)).await;
        // Second call without inputs should hit the cache and return the same.
        let m2 = cache.lookup_or_compute("AAPL", None).await;
        assert_eq!(m1, m2);
        assert_eq!(m1.momentum, "momentum_high");
        assert_eq!(m1.value, "value_high");
    }
}
