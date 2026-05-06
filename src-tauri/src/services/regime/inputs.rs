//! Phase 9 — read raw regime inputs from `bars_cache` via the
//! backtester's `BarsReader` trait seam. Lives behind the same trait
//! the backtester uses so tests can inject canned bar series without
//! standing up SQLite.
//!
//! Inputs gathered:
//!   - SPY 50-DMA / 200-DMA + slope of the 50-DMA
//!   - VIX last close + 5-day change (best-effort: VIX may not be
//!     pre-seeded in bars_cache; fallback to `None` and the
//!     classifier's vol axis defaults to `Normal`)
//!   - Breadth proxy: % of the universe trading above its own 50-DMA
//!   - 20-day rolling avg pairwise correlation of daily returns across
//!     the universe
//!
//! Universe is the embedded SP500-ish list in [`UNIVERSE`] — same
//! shape as `services::portfolio_risk::sector_map`'s static fallback.
//! Phase 9 ships this list deliberately small (~50 names) so the
//! "fresh-bar coverage >= 80%" requirement is reachable from a
//! lightly-primed bars_cache; growing to top-200 SP500 is an operator
//! task tracked in `loop/plan/QUESTIONS.md`.
//!
//! Survivorship caveat: the universe is fixed at compile time and
//! doesn't reflect SP500 add/drop history. Acceptable for live
//! classification (what's the regime *now*); for backtests, fix the
//! universe as-of the test date — same gotcha called out in the
//! phase doc.

use std::sync::Arc;

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize};

use crate::ibkr::types::historical::{BarSize, HistoricalBar};
use crate::services::backtester::bars_reader::{bar_time_utc, BarsReader};
use crate::storage::error::StorageError;

/// Compile-time universe used for breadth + correlation. Curated from
/// the SP500-by-marketcap front-half plus a few macro proxies so the
/// classifier still produces meaningful breadth even when only a
/// handful of names have fresh bars. Growing to top-200 SP500 is
/// tracked in QUESTIONS.md.
pub const UNIVERSE: &[&str] = &[
    "AAPL", "MSFT", "NVDA", "AMZN", "GOOGL", "META", "TSLA", "BRK.B", "AVGO", "JPM", "WMT", "LLY",
    "ORCL", "MA", "V", "XOM", "UNH", "COST", "HD", "PG", "JNJ", "BAC", "NFLX", "ABBV", "KO", "MRK",
    "CVX", "AMD", "PEP", "ADBE", "CSCO", "MCD", "CRM", "ACN", "TMO", "WFC", "ABT", "DIS", "GS",
    "INTC", "NOW", "QCOM", "IBM", "LIN", "TXN", "PM", "MS", "RTX", "CAT", "HON",
];

/// Default lookback for the daily-bar reader. 220 trading days covers
/// the 200-DMA computation with ~20 bars of warm-up.
const DAILY_LOOKBACK_DAYS: i64 = 260;
/// Window for cross-sectional correlation.
const CORR_WINDOW: usize = 20;
/// Window for breadth (% above 50-DMA).
const BREADTH_MA_WINDOW: usize = 50;
/// Coverage threshold for breadth/correlation. Under this, the
/// classifier defaults the affected axis to its neutral value.
const COVERAGE_FRESH_THRESHOLD: f64 = 0.80;
/// "Fresh" means a daily bar within this many days of `now`. Loose
/// enough to tolerate a long weekend / holiday-heavy week without
/// false-flagging the universe as stale.
const FRESH_BAR_MAX_AGE_DAYS: i64 = 7;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegimeInputs {
    pub spy: Option<SpyInputs>,
    pub vix: Option<VixInputs>,
    pub breadth: Option<BreadthInputs>,
    pub corr: Option<CorrInputs>,
    /// Names of the input slots that were requested but couldn't be
    /// computed because of missing or stale data. Populated even when
    /// the axis fell back to a neutral value, so the audit JSON shows
    /// *why* the classifier defaulted.
    pub missing: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpyInputs {
    pub last_close: f64,
    pub ma50: f64,
    pub ma200: f64,
    /// Slope of the 50-DMA over the last 10 sessions, expressed as a
    /// fraction (e.g. 0.005 = 50bps over 10 sessions).
    pub ma50_slope_10d: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VixInputs {
    pub last_close: f64,
    pub change_5d: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BreadthInputs {
    pub pct_above_50ma: f64,
    pub coverage: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorrInputs {
    /// Mean of pairwise Pearson correlations of 20-day daily returns
    /// across the covered subset of the universe.
    pub avg_pairwise_corr: f64,
    pub coverage: f64,
}

pub struct InputGatherer {
    bars: Arc<dyn BarsReader>,
    universe: Vec<&'static str>,
}

impl InputGatherer {
    pub fn new(bars: Arc<dyn BarsReader>) -> Self {
        Self {
            bars,
            universe: UNIVERSE.to_vec(),
        }
    }

    pub fn with_universe(bars: Arc<dyn BarsReader>, universe: Vec<&'static str>) -> Self {
        Self { bars, universe }
    }

    /// Fetch all four input slots. Each is best-effort: a missing
    /// series degrades to `None` + a `missing` log entry. The whole
    /// call only errors when the bars-reader itself errors at the
    /// transport layer (e.g. db locked) — empty rows are normal.
    pub async fn gather(&self, now: DateTime<Utc>) -> Result<RegimeInputs, StorageError> {
        let mut missing: Vec<String> = Vec::new();

        let spy = match self.fetch_window("SPY", now, DAILY_LOOKBACK_DAYS).await? {
            Some(bars) => match compute_spy(&bars) {
                Some(s) => Some(s),
                None => {
                    missing.push("spy_insufficient_bars".to_string());
                    None
                }
            },
            None => {
                missing.push("spy".to_string());
                None
            }
        };

        let vix = match self.fetch_window("VIX", now, DAILY_LOOKBACK_DAYS).await? {
            Some(bars) => match compute_vix(&bars) {
                Some(v) => Some(v),
                None => {
                    missing.push("vix_insufficient_bars".to_string());
                    None
                }
            },
            None => {
                missing.push("vix".to_string());
                None
            }
        };

        let (breadth, corr) = self.compute_universe_aggregates(now, &mut missing).await?;

        Ok(RegimeInputs {
            spy,
            vix,
            breadth,
            corr,
            missing,
        })
    }

    async fn fetch_window(
        &self,
        symbol: &str,
        now: DateTime<Utc>,
        lookback_days: i64,
    ) -> Result<Option<Vec<HistoricalBar>>, StorageError> {
        let end_unix = now.timestamp();
        let start_unix = (now - ChronoDuration::days(lookback_days)).timestamp();
        let bars = self
            .bars
            .read_window(symbol, BarSize::Day1, start_unix, end_unix)
            .await?;
        if bars.is_empty() {
            Ok(None)
        } else {
            Ok(Some(bars))
        }
    }

    async fn compute_universe_aggregates(
        &self,
        now: DateTime<Utc>,
        missing: &mut Vec<String>,
    ) -> Result<(Option<BreadthInputs>, Option<CorrInputs>), StorageError> {
        let mut series: Vec<Vec<HistoricalBar>> = Vec::with_capacity(self.universe.len());
        let mut fresh_count = 0usize;
        let mut sufficient_for_corr = 0usize;
        let mut sufficient_for_breadth = 0usize;
        let mut above_50ma = 0usize;

        for symbol in &self.universe {
            let bars_opt = self.fetch_window(symbol, now, DAILY_LOOKBACK_DAYS).await?;
            let Some(bars) = bars_opt else {
                series.push(Vec::new());
                continue;
            };
            if !is_fresh(&bars, now) {
                series.push(Vec::new());
                continue;
            }
            fresh_count += 1;

            if bars.len() > BREADTH_MA_WINDOW {
                sufficient_for_breadth += 1;
                if let Some(ma50) = sma_last(&bars, BREADTH_MA_WINDOW) {
                    let last = bars.last().expect("non-empty").close;
                    if last > ma50 {
                        above_50ma += 1;
                    }
                }
            }
            if bars.len() > CORR_WINDOW {
                sufficient_for_corr += 1;
            }
            series.push(bars);
        }

        let universe_n = self.universe.len() as f64;
        let coverage = (fresh_count as f64) / universe_n;

        let breadth = if sufficient_for_breadth >= 1
            && (sufficient_for_breadth as f64) / universe_n >= COVERAGE_FRESH_THRESHOLD
        {
            Some(BreadthInputs {
                pct_above_50ma: (above_50ma as f64) / (sufficient_for_breadth as f64),
                coverage,
            })
        } else {
            missing.push(format!(
                "breadth_coverage_low ({}/{}, threshold {:.0}%)",
                sufficient_for_breadth,
                self.universe.len(),
                COVERAGE_FRESH_THRESHOLD * 100.0
            ));
            None
        };

        let corr = if sufficient_for_corr >= 5
            && (sufficient_for_corr as f64) / universe_n >= COVERAGE_FRESH_THRESHOLD
        {
            let corr_val = pairwise_corr_mean(&series, CORR_WINDOW);
            corr_val.map(|c| CorrInputs {
                avg_pairwise_corr: c,
                coverage,
            })
        } else {
            missing.push(format!(
                "corr_coverage_low ({}/{}, threshold {:.0}%)",
                sufficient_for_corr,
                self.universe.len(),
                COVERAGE_FRESH_THRESHOLD * 100.0
            ));
            None
        };

        Ok((breadth, corr))
    }
}

fn is_fresh(bars: &[HistoricalBar], now: DateTime<Utc>) -> bool {
    let Some(last) = bars.last() else {
        return false;
    };
    let Some(when) = bar_time_utc(last) else {
        return false;
    };
    let age = now.signed_duration_since(when);
    age <= ChronoDuration::days(FRESH_BAR_MAX_AGE_DAYS) && age >= ChronoDuration::zero()
}

pub fn sma_last(bars: &[HistoricalBar], window: usize) -> Option<f64> {
    if bars.len() < window || window == 0 {
        return None;
    }
    let slice = &bars[bars.len() - window..];
    let sum: f64 = slice.iter().map(|b| b.close).sum();
    Some(sum / window as f64)
}

pub fn sma_at(bars: &[HistoricalBar], window: usize, end_idx_exclusive: usize) -> Option<f64> {
    if end_idx_exclusive < window || window == 0 || end_idx_exclusive > bars.len() {
        return None;
    }
    let slice = &bars[end_idx_exclusive - window..end_idx_exclusive];
    let sum: f64 = slice.iter().map(|b| b.close).sum();
    Some(sum / window as f64)
}

fn compute_spy(bars: &[HistoricalBar]) -> Option<SpyInputs> {
    if bars.len() < 210 {
        return None;
    }
    let last_close = bars.last()?.close;
    let ma50 = sma_last(bars, 50)?;
    let ma200 = sma_last(bars, 200)?;
    // 10-session slope of the 50-DMA, expressed as fraction.
    let ma50_now = ma50;
    let ma50_then = sma_at(bars, 50, bars.len() - 10)?;
    let ma50_slope_10d = if ma50_then > 0.0 {
        (ma50_now - ma50_then) / ma50_then
    } else {
        0.0
    };
    Some(SpyInputs {
        last_close,
        ma50,
        ma200,
        ma50_slope_10d,
    })
}

fn compute_vix(bars: &[HistoricalBar]) -> Option<VixInputs> {
    if bars.len() < 6 {
        return None;
    }
    let last_close = bars.last()?.close;
    let prev_close = bars[bars.len() - 6].close;
    let change_5d = if prev_close > 0.0 {
        (last_close - prev_close) / prev_close
    } else {
        0.0
    };
    Some(VixInputs {
        last_close,
        change_5d,
    })
}

/// Mean of pairwise Pearson correlations over the most recent `window`
/// daily returns across `series`. Series with fewer than `window+1`
/// bars are skipped. Returns None when fewer than 2 series are
/// usable.
fn pairwise_corr_mean(series: &[Vec<HistoricalBar>], window: usize) -> Option<f64> {
    let mut returns: Vec<Vec<f64>> = Vec::new();
    for bars in series {
        if bars.len() < window + 1 {
            continue;
        }
        let slice = &bars[bars.len() - window - 1..];
        let mut rs = Vec::with_capacity(window);
        for pair in slice.windows(2) {
            let prev = pair[0].close;
            let now = pair[1].close;
            if prev > 0.0 {
                rs.push((now - prev) / prev);
            } else {
                rs.push(0.0);
            }
        }
        if rs.len() == window {
            returns.push(rs);
        }
    }
    if returns.len() < 2 {
        return None;
    }
    let mut total = 0.0;
    let mut pairs = 0;
    for i in 0..returns.len() {
        for j in (i + 1)..returns.len() {
            if let Some(c) = pearson(&returns[i], &returns[j]) {
                total += c;
                pairs += 1;
            }
        }
    }
    if pairs == 0 {
        None
    } else {
        Some(total / pairs as f64)
    }
}

fn pearson(a: &[f64], b: &[f64]) -> Option<f64> {
    let n = a.len();
    if n != b.len() || n < 2 {
        return None;
    }
    let mean_a = a.iter().sum::<f64>() / n as f64;
    let mean_b = b.iter().sum::<f64>() / n as f64;
    let mut num = 0.0;
    let mut denom_a = 0.0;
    let mut denom_b = 0.0;
    for i in 0..n {
        let da = a[i] - mean_a;
        let db = b[i] - mean_b;
        num += da * db;
        denom_a += da * da;
        denom_b += db * db;
    }
    let denom = (denom_a * denom_b).sqrt();
    if denom == 0.0 {
        None
    } else {
        Some(num / denom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar(close: f64) -> HistoricalBar {
        HistoricalBar {
            time: "20260101".to_string(),
            open: close,
            high: close,
            low: close,
            close,
            volume: 1,
            wap: close,
            count: 0,
        }
    }

    #[test]
    fn sma_window_handles_short_series() {
        let bars = vec![bar(1.0), bar(2.0), bar(3.0)];
        assert!(sma_last(&bars, 5).is_none());
        assert_eq!(sma_last(&bars, 3).unwrap(), 2.0);
    }

    #[test]
    fn pearson_perfect_correlation() {
        let a = vec![1.0, 2.0, 3.0, 4.0];
        let b = vec![2.0, 4.0, 6.0, 8.0];
        let c = pearson(&a, &b).unwrap();
        assert!((c - 1.0).abs() < 1e-9);
    }

    #[test]
    fn pearson_perfect_anticorrelation() {
        let a = vec![1.0, 2.0, 3.0, 4.0];
        let b = vec![4.0, 3.0, 2.0, 1.0];
        let c = pearson(&a, &b).unwrap();
        assert!((c - (-1.0)).abs() < 1e-9);
    }

    #[test]
    fn pearson_zero_variance_returns_none() {
        let a = vec![1.0, 1.0, 1.0];
        let b = vec![2.0, 2.0, 2.0];
        assert!(pearson(&a, &b).is_none());
    }

    #[test]
    fn pairwise_corr_mean_skips_short_series() {
        let s1 = (0..25).map(|i| bar(100.0 + i as f64)).collect();
        let s2 = (0..25).map(|i| bar(50.0 + i as f64 * 0.5)).collect();
        let s3 = vec![bar(10.0)]; // too short, skipped
        let mean = pairwise_corr_mean(&[s1, s2, s3], 20).unwrap();
        // Both surviving series rise monotonically — high positive corr.
        assert!(mean > 0.95);
    }
}
