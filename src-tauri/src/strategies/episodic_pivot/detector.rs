//! Bidirectional Episodic Pivot detector.
//!
//! Fires on news-driven gaps where sentiment polarity aligns with — or
//! deliberately fades — the gap, provided first-30-min volume confirms
//! institutional flow against the prior day's full-session volume.
//!
//! Direction logic:
//! - gap up + bullish news → Long (continuation)
//! - gap down + bearish news → Short (continuation)
//! - gap up + bearish news → Short (fade)
//! - gap down + bullish news → no setup
//!
//! Stops: long → previous day's close (the pre-gap close). Short → highest
//! intraday high seen so far (the gap-day high). Targets are 2R / 3R.

use async_trait::async_trait;
use chrono::Utc;
use serde_json::json;

use crate::ibkr::types::{BarSize, NewsItem, NewsTone, NewsVerdict, StrategyTag};
use crate::strategies::config::EpisodicPivotCfg;
use crate::strategies::{
    targets_for_risk_profile, DetectorError, Direction, MarketContext, SetupCandidate,
    StrategyDetector,
};

const MIN_LOOKBACK_DAYS: u32 = 5;
const MIN_DAILY_BARS: usize = 2; // need today + yesterday at minimum
/// Conviction-normalization upper bounds. The lower bounds come from the
/// detector config so the fire-or-skip threshold and the conviction floor
/// stay aligned.
const MAX_GAP_PCT: f64 = 0.10;
const MAX_SENTIMENT: f64 = 0.50;
const MAX_VOLUME_RATIO: f64 = 3.0;
/// First 30 minutes of session @ 15-min resolution = 2 bars.
const FIRST_30_MIN_BARS: usize = 2;
/// Sentiment magnitude assigned when polarity comes from a [`NewsVerdict`]
/// rather than per-item AV scores. Solidly inside the
/// `min_sentiment_abs..MAX_SENTIMENT` band so the conviction blend stays
/// well-defined.
const VERDICT_SENTIMENT_MAGNITUDE: f64 = 0.325;

#[derive(Debug, Clone, Default)]
pub struct EpisodicPivotDetector {
    cfg: EpisodicPivotCfg,
}

impl EpisodicPivotDetector {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_config(cfg: EpisodicPivotCfg) -> Self {
        Self { cfg }
    }

    /// Returns `(sentiment_score, relevance_score)` for the news item with
    /// the highest per-symbol relevance, or `None` if no item carries a
    /// `ticker_sentiment` entry for `symbol`.
    fn pick_sentiment(symbol: &str, news: &[NewsItem]) -> Option<(f64, f64)> {
        news.iter()
            .filter_map(|item| {
                let ts = item
                    .ticker_sentiment
                    .iter()
                    .find(|ts| ts.ticker.eq_ignore_ascii_case(symbol))?;
                Some((ts.ticker_sentiment_score, ts.relevance_score))
            })
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
    }

    /// Polarity-only signal derived from a [`NewsVerdict`]. Bullish /
    /// bearish map to a fixed magnitude inside the conviction band so
    /// the rest of the pipeline can keep treating sentiment as a
    /// signed scalar; neutral collapses to `None` so the caller
    /// short-circuits before falling through to AV.
    fn sentiment_from_verdict(verdict: &NewsVerdict) -> Option<f64> {
        match verdict.tone {
            NewsTone::Bullish => Some(VERDICT_SENTIMENT_MAGNITUDE),
            NewsTone::Bearish => Some(-VERDICT_SENTIMENT_MAGNITUDE),
            NewsTone::Neutral => None,
        }
    }

    fn normalize(x: f64, lo: f64, hi: f64) -> f64 {
        if hi <= lo {
            return 0.0;
        }
        ((x - lo) / (hi - lo)).clamp(0.0, 1.0)
    }

    fn conviction(&self, gap_abs: f64, sent_abs: f64, vol_ratio: f64) -> f64 {
        let g = Self::normalize(gap_abs, self.cfg.min_gap_pct, MAX_GAP_PCT);
        let s = Self::normalize(sent_abs, self.cfg.min_sentiment_abs, MAX_SENTIMENT);
        let v = Self::normalize(vol_ratio, self.cfg.min_volume_ratio, MAX_VOLUME_RATIO);
        (0.4 * g + 0.4 * s + 0.2 * v).clamp(0.0, 1.0)
    }
}

#[async_trait]
impl StrategyDetector for EpisodicPivotDetector {
    fn name(&self) -> &'static str {
        "episodic_pivot"
    }
    fn tag(&self) -> StrategyTag {
        StrategyTag::EpisodicPivot
    }
    fn timeframe(&self) -> BarSize {
        BarSize::Min15
    }
    fn min_lookback_days(&self) -> u32 {
        MIN_LOOKBACK_DAYS
    }

    async fn evaluate(
        &self,
        ctx: &MarketContext<'_>,
    ) -> Result<Option<SetupCandidate>, DetectorError> {
        let bars = ctx.daily_bars;
        if bars.len() < MIN_DAILY_BARS {
            return Err(DetectorError::InsufficientBars {
                needed: MIN_DAILY_BARS,
                available: bars.len(),
            });
        }
        let n = bars.len();
        let yesterday = &bars[n - 2];
        let today = &bars[n - 1];

        // Compute gap vs prior-day close.
        if yesterday.close == 0.0 {
            return Ok(None);
        }
        let gap_pct = (today.open - yesterday.close) / yesterday.close;
        if gap_pct.abs() < self.cfg.min_gap_pct {
            return Ok(None);
        }

        // Intraday bars are required for the volume check + short stop.
        let intraday = ctx
            .intraday_bars
            .ok_or(DetectorError::IntradayBarsRequired)?;

        // Prefer the LLM-derived NewsVerdict (Phase 19) when present; it
        // reasons over the full headline set instead of one max-relevance
        // AV item. Fall back to per-item AV sentiment when no verdict
        // exists (LLM disabled, budget exhausted, or first pass before
        // the interpreter has run).
        let sentiment = match ctx.news_verdict.and_then(Self::sentiment_from_verdict) {
            Some(s) => s,
            None => {
                let (s, _relevance) = match Self::pick_sentiment(ctx.symbol, ctx.recent_news) {
                    Some(v) => v,
                    None => return Ok(None),
                };
                if s.abs() < self.cfg.min_sentiment_abs {
                    return Ok(None);
                }
                s
            }
        };

        // Direction decision: continuation up, continuation down, or gap-up
        // fade. Gap-down with bullish sentiment is intentionally not modeled.
        let direction = match (gap_pct > 0.0, sentiment > 0.0) {
            (true, true) => Direction::Long,
            (false, false) => Direction::Short, // continuation down
            (true, false) => Direction::Short,  // fade short
            (false, true) => return Ok(None),
        };

        // Volume confirmation: first 30-min sum vs prior-day total.
        let take_n = FIRST_30_MIN_BARS.min(intraday.len());
        let first_30min_vol: i64 = intraday.iter().take(take_n).map(|b| b.volume).sum();
        let prior_day_vol = yesterday.volume;
        let vol_ratio = if prior_day_vol > 0 {
            first_30min_vol as f64 / prior_day_vol as f64
        } else {
            0.0
        };
        if vol_ratio < self.cfg.min_volume_ratio {
            return Ok(None);
        }

        // Trigger = today's RTH open.
        let trigger_price = today.open;
        let stop_price = match direction {
            Direction::Long => yesterday.close,
            Direction::Short => intraday
                .iter()
                .map(|b| b.high)
                .fold(f64::NEG_INFINITY, f64::max),
        };

        // Reject degenerate setups where the chosen stop is on the wrong side
        // of trigger (e.g., gap-up fade where intraday high hasn't crossed open).
        match direction {
            Direction::Long => {
                if trigger_price <= stop_price {
                    return Ok(None);
                }
            }
            Direction::Short => {
                if stop_price <= trigger_price {
                    return Ok(None);
                }
            }
        }

        let targets = targets_for_risk_profile(direction, trigger_price, stop_price)
            .map_err(|e| DetectorError::Internal(e.into()))?;

        let raw_signals = json!({
            "gap_pct": gap_pct,
            "sentiment_score": sentiment,
            "volume_ratio": vol_ratio,
            "first_30min_volume": first_30min_vol,
            "prior_day_volume": prior_day_vol,
        });

        Ok(Some(SetupCandidate {
            strategy: "episodic_pivot",
            tag: StrategyTag::EpisodicPivot,
            direction,
            conviction_signal: self.conviction(gap_pct.abs(), sentiment.abs(), vol_ratio),
            trigger_price,
            stop_price,
            targets,
            raw_signals,
            timeframe: BarSize::Min15,
            detected_at: Utc::now(),
        }))
    }
}
