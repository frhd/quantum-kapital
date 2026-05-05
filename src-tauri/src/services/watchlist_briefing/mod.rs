//! `watchlist_briefing` — fan-out composer that produces a single
//! `WatchlistBriefing` per call by issuing the six per-symbol read
//! services in parallel and packaging the results with a per-symbol
//! error envelope.

use std::future::Future;
use std::pin::Pin;

use chrono::Utc;
use futures::stream::{FuturesUnordered, StreamExt};
use serde_json::Value;

pub mod types;
#[cfg(test)]
mod tests;

pub use types::{SymbolBriefing, WatchlistBriefing};

pub type FetchResult = Result<Value, String>;
pub type Future01<'a> = Pin<Box<dyn Future<Output = FetchResult> + Send + 'a>>;
pub type SymbolFetcher = Box<dyn Fn(&str) -> Future01<'static> + Send + Sync>;
pub type BarsFetcher = Box<dyn Fn(&str, &str, u32) -> Future01<'static> + Send + Sync>;

pub struct BriefingFetchers {
    pub fetch_quote: SymbolFetcher,
    pub fetch_bars: BarsFetcher,
    pub fetch_news: SymbolFetcher,
    pub fetch_sentiment: SymbolFetcher,
    pub fetch_setups: SymbolFetcher,
    pub fetch_fundamentals: SymbolFetcher,
}

#[derive(Debug, Clone)]
pub struct BriefingOpts {
    pub lookback_days: u32,
    pub bars_size: String,
    pub news_max_age_secs: u32,
    pub concurrency: usize,
}

impl Default for BriefingOpts {
    fn default() -> Self {
        Self {
            lookback_days: 15,
            bars_size: "1d".into(),
            news_max_age_secs: 3600,
            concurrency: 4,
        }
    }
}

pub async fn compose(
    symbols: Vec<String>,
    opts: BriefingOpts,
    f: &BriefingFetchers,
) -> WatchlistBriefing {
    let concurrency = opts.concurrency.max(1);
    let mut pending = symbols.iter().cloned();
    let mut tasks: FuturesUnordered<_> = FuturesUnordered::new();

    for _ in 0..concurrency {
        if let Some(sym) = pending.next() {
            tasks.push(spawn_briefing(sym, &opts, f));
        }
    }

    let mut items: Vec<SymbolBriefing> = Vec::with_capacity(symbols.len());
    while let Some(item) = tasks.next().await {
        items.push(item);
        if let Some(sym) = pending.next() {
            tasks.push(spawn_briefing(sym, &opts, f));
        }
    }
    items.sort_by(|a, b| a.symbol.cmp(&b.symbol));
    WatchlistBriefing {
        as_of: Utc::now().timestamp(),
        symbols,
        items,
    }
}

fn spawn_briefing<'a>(
    sym: String,
    opts: &'a BriefingOpts,
    f: &'a BriefingFetchers,
) -> Pin<Box<dyn Future<Output = SymbolBriefing> + Send + 'a>> {
    Box::pin(async move {
        let (q, b, n, sent, set, fund) = tokio::join!(
            (f.fetch_quote)(&sym),
            (f.fetch_bars)(&sym, &opts.bars_size, opts.lookback_days),
            (f.fetch_news)(&sym),
            (f.fetch_sentiment)(&sym),
            (f.fetch_setups)(&sym),
            (f.fetch_fundamentals)(&sym),
        );
        into_briefing(sym, q, b, n, sent, set, fund)
    })
}

fn into_briefing(
    symbol: String,
    quote: FetchResult,
    bars: FetchResult,
    news: FetchResult,
    sentiment: FetchResult,
    setups: FetchResult,
    fundamentals: FetchResult,
) -> SymbolBriefing {
    let mut errors = Vec::new();
    let q = field("quote", quote, &mut errors);
    let b = field("bars", bars, &mut errors);
    let n = field("news", news, &mut errors);
    let s = field("sentiment", sentiment, &mut errors);
    let st = field("setups", setups, &mut errors);
    let fu = field("fundamentals", fundamentals, &mut errors);
    SymbolBriefing {
        symbol,
        quote: q,
        bars: b,
        news: n,
        sentiment: s,
        setups: st,
        fundamentals: fu,
        errors,
    }
}

fn field(name: &str, r: FetchResult, errors: &mut Vec<String>) -> Option<Value> {
    match r {
        Ok(v) => Some(v),
        Err(e) => {
            errors.push(format!("{name}: {e}"));
            None
        }
    }
}
