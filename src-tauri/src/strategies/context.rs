use chrono::{DateTime, Utc};

use crate::ibkr::types::{FundamentalData, HistoricalBar, MarketDataSnapshot, NewsItem};

#[derive(Debug)]
pub struct MarketContext<'a> {
    pub symbol: &'a str,
    pub daily_bars: &'a [HistoricalBar],
    pub intraday_bars: Option<&'a [HistoricalBar]>,
    pub fundamentals: Option<&'a FundamentalData>,
    pub recent_news: &'a [NewsItem],
    pub current_quote: Option<&'a MarketDataSnapshot>,
    pub now: DateTime<Utc>,
}
