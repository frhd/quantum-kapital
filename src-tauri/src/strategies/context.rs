use chrono::{DateTime, Utc};

use crate::ibkr::types::{
    DataTier, FundamentalData, HistoricalBar, MarketDataSnapshot, NewsItem, NewsVerdict,
};

#[derive(Debug)]
pub struct MarketContext<'a> {
    pub symbol: &'a str,
    pub daily_bars: &'a [HistoricalBar],
    pub intraday_bars: Option<&'a [HistoricalBar]>,
    pub fundamentals: Option<&'a FundamentalData>,
    pub recent_news: &'a [NewsItem],
    /// LLM-derived per-symbol news verdict (Phase 19). When present,
    /// detectors should prefer it over raw AV sentiment for polarity
    /// decisions. `None` falls back to per-item sentiment.
    pub news_verdict: Option<&'a NewsVerdict>,
    pub current_quote: Option<&'a MarketDataSnapshot>,
    /// Detected market-data tier for the active connection. Future
    /// real-time-only detectors should return `Ok(None)` when this
    /// isn't `RealTime`. Defaults to `Unknown` so detectors that
    /// don't care about tier keep working unchanged.
    pub data_tier: DataTier,
    pub now: DateTime<Utc>,
}
